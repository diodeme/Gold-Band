use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::acp::events::load_timeline_items;
use crate::storage::{read_json, write_json};

pub const PROMPT_QUEUE_FILE_NAME: &str = "acp.prompt-queue.json";
pub const MAX_QUEUED_PROMPTS: usize = 10;
pub const AUTO_DISPATCH_USER_PRIORITY_GRACE_MS: u64 = 600;

static PROMPT_QUEUE_LOCKS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_DISPATCHES: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static AUTO_DISPATCH_SUSPENSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueuedPromptState {
    Queued,
    Dispatching,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPrompt {
    pub id: String,
    pub prompt_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_paths: Vec<String>,
    pub created_at: String,
    pub state: QueuedPromptState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueue {
    pub version: u32,
    pub revision: u64,
    #[serde(default)]
    pub auto_dispatch_suspended: bool,
    #[serde(default)]
    pub items: Vec<QueuedPrompt>,
}

impl Default for PromptQueue {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            auto_dispatch_suspended: false,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PromptQueueError {
    #[error("prompt queue is full")]
    Full,
    #[error("queued prompt was not found")]
    NotFound,
    #[error("queued prompt is already being dispatched")]
    Dispatching,
    #[error("queued prompt content is empty")]
    Empty,
    #[error("prompt queue storage is unavailable")]
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoClaimResult {
    Claimed(QueuedPrompt),
    Empty,
    Preempted,
    Suspended,
}

pub fn queue_path(attempt_dir: &Utf8Path) -> Utf8PathBuf {
    attempt_dir.join(PROMPT_QUEUE_FILE_NAME)
}

pub fn load_prompt_queue(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    with_queue_lock(attempt_dir, || load_and_reconcile_unlocked(attempt_dir))
}

pub fn enqueue_prompt(
    attempt_dir: &Utf8Path,
    content: String,
    attachment_paths: Vec<String>,
) -> Result<QueuedPrompt, PromptQueueError> {
    if content.trim().is_empty() {
        return Err(PromptQueueError::Empty);
    }
    with_typed_queue_lock(attempt_dir, || {
        let mut queue =
            load_and_reconcile_unlocked(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        if queue.items.len() >= MAX_QUEUED_PROMPTS {
            return Err(PromptQueueError::Full);
        }
        let id = format!("queued-{}", Uuid::new_v4().simple());
        let item = QueuedPrompt {
            prompt_id: format!("turn-{id}"),
            id,
            content,
            attachment_paths,
            created_at: chrono::Utc::now().to_rfc3339(),
            state: QueuedPromptState::Queued,
        };
        queue.items.push(item.clone());
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        Ok(item)
    })
}

pub fn update_queued_prompt(
    attempt_dir: &Utf8Path,
    item_id: &str,
    content: String,
) -> Result<PromptQueue, PromptQueueError> {
    if content.trim().is_empty() {
        return Err(PromptQueueError::Empty);
    }
    with_typed_queue_lock(attempt_dir, || {
        let mut queue =
            load_and_reconcile_unlocked(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        let item = queue
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .ok_or(PromptQueueError::NotFound)?;
        if item.state == QueuedPromptState::Dispatching {
            return Err(PromptQueueError::Dispatching);
        }
        item.content = content;
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        Ok(queue)
    })
}

pub fn delete_queued_prompt(
    attempt_dir: &Utf8Path,
    item_id: &str,
) -> Result<PromptQueue, PromptQueueError> {
    with_typed_queue_lock(attempt_dir, || {
        let mut queue =
            load_and_reconcile_unlocked(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        let index = queue
            .items
            .iter()
            .position(|item| item.id == item_id)
            .ok_or(PromptQueueError::NotFound)?;
        if queue.items[index].state == QueuedPromptState::Dispatching {
            return Err(PromptQueueError::Dispatching);
        }
        queue.items.remove(index);
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        Ok(queue)
    })
}

/// Records a real user submission so a pending automatic claim can observe
/// the revision change and yield. The queue is unchanged.
pub fn mark_user_priority(attempt_dir: &Utf8Path) -> Result<u64> {
    with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        queue.auto_dispatch_suspended = false;
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        clear_auto_dispatch_suspension(attempt_dir)?;
        Ok(queue.revision)
    })
}

pub fn suspend_auto_dispatch(attempt_dir: &Utf8Path) -> Result<u64> {
    request_auto_dispatch_suspension(attempt_dir)?;
    with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        queue.auto_dispatch_suspended = true;
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(queue.revision)
    })
}

pub fn request_auto_dispatch_suspension(attempt_dir: &Utf8Path) -> Result<()> {
    AUTO_DISPATCH_SUSPENSIONS
        .lock()
        .map_err(|_| anyhow!("prompt queue suspension registry poisoned"))?
        .insert(queue_path(attempt_dir).to_string());
    Ok(())
}

pub fn clear_auto_dispatch_suspension(attempt_dir: &Utf8Path) -> Result<()> {
    AUTO_DISPATCH_SUSPENSIONS
        .lock()
        .map_err(|_| anyhow!("prompt queue suspension registry poisoned"))?
        .remove(&queue_path(attempt_dir).to_string());
    Ok(())
}

pub fn auto_dispatch_is_suspended(attempt_dir: &Utf8Path) -> bool {
    AUTO_DISPATCH_SUSPENSIONS
        .lock()
        .is_ok_and(|suspensions| suspensions.contains(&queue_path(attempt_dir).to_string()))
}

pub fn current_revision(attempt_dir: &Utf8Path) -> Result<u64> {
    Ok(load_prompt_queue(attempt_dir)?.revision)
}

pub fn claim_next_for_auto_dispatch(
    attempt_dir: &Utf8Path,
    expected_revision: u64,
) -> Result<AutoClaimResult> {
    with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        if queue.revision != expected_revision {
            return Ok(AutoClaimResult::Preempted);
        }
        if queue.auto_dispatch_suspended || auto_dispatch_is_suspended(attempt_dir) {
            return Ok(AutoClaimResult::Suspended);
        }
        let Some(item) = queue
            .items
            .iter_mut()
            .find(|item| item.state == QueuedPromptState::Queued)
        else {
            return Ok(AutoClaimResult::Empty);
        };
        item.state = QueuedPromptState::Dispatching;
        let claimed = item.clone();
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        mark_dispatch_active(attempt_dir, &claimed.id)?;
        Ok(AutoClaimResult::Claimed(claimed))
    })
}

pub fn claim_queued_prompt(
    attempt_dir: &Utf8Path,
    item_id: &str,
) -> Result<QueuedPrompt, PromptQueueError> {
    with_typed_queue_lock(attempt_dir, || {
        let mut queue =
            load_and_reconcile_unlocked(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        let item = queue
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .ok_or(PromptQueueError::NotFound)?;
        if item.state == QueuedPromptState::Dispatching {
            return Err(PromptQueueError::Dispatching);
        }
        queue.auto_dispatch_suspended = false;
        clear_auto_dispatch_suspension(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        item.state = QueuedPromptState::Dispatching;
        let claimed = item.clone();
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        mark_dispatch_active(attempt_dir, &claimed.id).map_err(|_| PromptQueueError::Storage)?;
        Ok(claimed)
    })
}

pub fn complete_queued_prompt(attempt_dir: &Utf8Path, item_id: &str) -> Result<PromptQueue> {
    let result = with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        queue.items.retain(|item| item.id != item_id);
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(queue)
    });
    clear_dispatch_active(attempt_dir, item_id)?;
    result
}

pub fn release_queued_prompt(attempt_dir: &Utf8Path, item_id: &str) -> Result<PromptQueue> {
    let result = with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        if let Some(item) = queue.items.iter_mut().find(|item| item.id == item_id) {
            item.state = QueuedPromptState::Queued;
        }
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(queue)
    });
    clear_dispatch_active(attempt_dir, item_id)?;
    result
}

pub fn release_dispatching_prompts(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let result = with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        for item in &mut queue.items {
            if item.state == QueuedPromptState::Dispatching {
                item.state = QueuedPromptState::Queued;
            }
        }
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(queue)
    });
    clear_attempt_dispatches(attempt_dir)?;
    result
}

/// Finalizes an in-process dispatch only after its stable prompt id is present
/// in the durable timeline. This is used by asynchronous runtime continuation.
pub fn complete_accepted_dispatches(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let result = with_queue_lock(attempt_dir, || {
        let mut queue = load_and_reconcile_unlocked(attempt_dir)?;
        let accepted_prompt_ids = accepted_prompt_ids(attempt_dir);
        let completed_ids = queue
            .items
            .iter()
            .filter(|item| {
                item.state == QueuedPromptState::Dispatching
                    && accepted_prompt_ids.contains(&item.prompt_id)
            })
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if completed_ids.is_empty() {
            return Ok((queue, completed_ids));
        }
        queue.items.retain(|item| !completed_ids.contains(&item.id));
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok((queue, completed_ids))
    });
    let (queue, completed_ids) = result?;
    for item_id in completed_ids {
        clear_dispatch_active(attempt_dir, &item_id)?;
    }
    Ok(queue)
}

fn load_and_reconcile_unlocked(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let path = queue_path(attempt_dir);
    let mut queue = if path.exists() {
        read_json::<PromptQueue>(&path)?
    } else {
        PromptQueue::default()
    };
    let accepted_prompt_ids = accepted_prompt_ids(attempt_dir);
    let mut changed = false;
    queue.items.retain_mut(|item| {
        if item.state != QueuedPromptState::Dispatching {
            return true;
        }
        if dispatch_is_active(attempt_dir, &item.id) {
            return true;
        }
        changed = true;
        if accepted_prompt_ids.contains(&item.prompt_id) {
            false
        } else {
            item.state = QueuedPromptState::Queued;
            true
        }
    });
    if changed {
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
    }
    Ok(queue)
}

fn accepted_prompt_ids(attempt_dir: &Utf8Path) -> HashSet<String> {
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    if !timeline_path.exists() {
        return HashSet::new();
    }
    load_timeline_items(&timeline_path)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|event| {
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("promptId"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn dispatch_key(attempt_dir: &Utf8Path, item_id: &str) -> String {
    format!("{}::{item_id}", queue_path(attempt_dir))
}

fn mark_dispatch_active(attempt_dir: &Utf8Path, item_id: &str) -> Result<()> {
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .insert(dispatch_key(attempt_dir, item_id));
    Ok(())
}

fn clear_dispatch_active(attempt_dir: &Utf8Path, item_id: &str) -> Result<()> {
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .remove(&dispatch_key(attempt_dir, item_id));
    Ok(())
}

fn clear_attempt_dispatches(attempt_dir: &Utf8Path) -> Result<()> {
    let prefix = format!("{}::", queue_path(attempt_dir));
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .retain(|key| !key.starts_with(&prefix));
    Ok(())
}

fn dispatch_is_active(attempt_dir: &Utf8Path, item_id: &str) -> bool {
    ACTIVE_DISPATCHES
        .lock()
        .is_ok_and(|dispatches| dispatches.contains(&dispatch_key(attempt_dir, item_id)))
}

fn persist_mutation_unlocked(attempt_dir: &Utf8Path, queue: &mut PromptQueue) -> Result<()> {
    queue.revision = queue.revision.saturating_add(1);
    write_json(&queue_path(attempt_dir), queue)
}

fn queue_lock(attempt_dir: &Utf8Path) -> Result<Arc<Mutex<()>>> {
    let key = queue_path(attempt_dir).to_string();
    let mut locks = PROMPT_QUEUE_LOCKS
        .lock()
        .map_err(|_| anyhow!("prompt queue lock registry poisoned"))?;
    Ok(locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn with_queue_lock<T>(attempt_dir: &Utf8Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock = queue_lock(attempt_dir)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("prompt queue lock poisoned"))?;
    operation()
}

fn with_typed_queue_lock<T>(
    attempt_dir: &Utf8Path,
    operation: impl FnOnce() -> Result<T, PromptQueueError>,
) -> Result<T, PromptQueueError> {
    let lock = queue_lock(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
    let _guard = lock.lock().map_err(|_| PromptQueueError::Storage)?;
    operation()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt_dir(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().join("attempt-001")).unwrap()
    }

    #[test]
    fn queue_preserves_position_when_editing_and_limits_capacity() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(&dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let queue = update_queued_prompt(&dir, &first.id, "edited".to_string()).unwrap();
        assert_eq!(queue.items[0].id, first.id);
        assert_eq!(queue.items[0].content, "edited");
        assert_eq!(queue.items[1].id, second.id);

        for index in 2..MAX_QUEUED_PROMPTS {
            enqueue_prompt(&dir, format!("item-{index}"), Vec::new()).unwrap();
        }
        assert_eq!(
            enqueue_prompt(&dir, "overflow".to_string(), Vec::new()),
            Err(PromptQueueError::Full)
        );
    }

    #[test]
    fn real_user_revision_preempts_automatic_claim() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        enqueue_prompt(&dir, "queued".to_string(), Vec::new()).unwrap();
        let revision = current_revision(&dir).unwrap();
        mark_user_priority(&dir).unwrap();
        assert_eq!(
            claim_next_for_auto_dispatch(&dir, revision).unwrap(),
            AutoClaimResult::Preempted
        );
        assert_eq!(load_prompt_queue(&dir).unwrap().items.len(), 1);
    }

    #[test]
    fn manual_use_removes_selected_item_without_reordering_remaining_items() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(&dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let third = enqueue_prompt(&dir, "third".to_string(), Vec::new()).unwrap();
        let claimed = claim_queued_prompt(&dir, &second.id).unwrap();
        assert_eq!(claimed.id, second.id);
        let queue = complete_queued_prompt(&dir, &second.id).unwrap();
        assert_eq!(
            queue
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec![first.id.as_str(), third.id.as_str()]
        );
    }

    #[test]
    fn live_projection_does_not_restore_an_in_process_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "queued".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&dir, &item.id).unwrap();

        let queue = load_prompt_queue(&dir).unwrap();
        assert_eq!(queue.items[0].state, QueuedPromptState::Dispatching);
        release_queued_prompt(&dir, &item.id).unwrap();
        assert_eq!(
            load_prompt_queue(&dir).unwrap().items[0].state,
            QueuedPromptState::Queued
        );
    }

    #[test]
    fn orphaned_dispatch_is_restored_after_process_state_is_lost() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "survives restart".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&dir, &item.id).unwrap();
        clear_dispatch_active(&dir, &item.id).unwrap();

        let queue = load_prompt_queue(&dir).unwrap();
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].content, "survives restart");
        assert_eq!(queue.items[0].state, QueuedPromptState::Queued);
    }

    #[test]
    fn stop_suspends_auto_dispatch_until_a_real_user_submission_resumes_it() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        enqueue_prompt(&dir, "wait after stop".to_string(), Vec::new()).unwrap();
        suspend_auto_dispatch(&dir).unwrap();
        let suspended_revision = current_revision(&dir).unwrap();

        assert_eq!(
            claim_next_for_auto_dispatch(&dir, suspended_revision).unwrap(),
            AutoClaimResult::Suspended
        );
        assert_eq!(load_prompt_queue(&dir).unwrap().items.len(), 1);

        let resumed_revision = mark_user_priority(&dir).unwrap();
        assert!(matches!(
            claim_next_for_auto_dispatch(&dir, resumed_revision).unwrap(),
            AutoClaimResult::Claimed(_)
        ));
    }

    #[test]
    fn manual_use_resumes_a_queue_suspended_by_stop() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "manual".to_string(), Vec::new()).unwrap();
        suspend_auto_dispatch(&dir).unwrap();

        claim_queued_prompt(&dir, &item.id).unwrap();
        assert!(!load_prompt_queue(&dir).unwrap().auto_dispatch_suspended);
    }
}
