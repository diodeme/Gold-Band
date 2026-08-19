use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{ConversationPromptInput, UserPromptQuote};
use crate::storage::{read_json, write_json};

pub const PROMPT_QUEUE_FILE_NAME: &str = "acp.prompt-queue.json";
pub const MAX_QUEUED_PROMPTS: usize = 10;
pub const AUTO_DISPATCH_USER_PRIORITY_GRACE_MS: u64 = 600;
const PROMPT_QUEUE_VERSION: u32 = 4;

static PROMPT_QUEUE_LOCKS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ACTIVE_DISPATCHES: LazyLock<Mutex<HashMap<String, ActiveDispatch>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static AUTO_DISPATCH_SUSPENSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static AUTO_DISPATCH_REPLY_BATCHES: LazyLock<Mutex<HashMap<String, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveDispatch {
    queue_path: String,
    item_id: String,
}

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
    pub quotes: Vec<UserPromptQuote>,
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
            version: PROMPT_QUEUE_VERSION,
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
    #[error("prompt queue revision is stale")]
    RevisionConflict,
    #[error("prompt queue order is invalid")]
    InvalidOrder,
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

/// Resolution of a dispatch whose prior logical turn is already terminal.
/// A prompt present in the timeline is accepted and must leave the queue;
/// otherwise it receives a fresh execution identity while retaining the
/// user's durable queue item and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalDispatchRecovery {
    Reclaimed(QueuedPrompt),
    AlreadyAccepted,
    Missing,
}

pub fn queue_path(attempt_dir: &Utf8Path) -> Utf8PathBuf {
    attempt_dir.join(PROMPT_QUEUE_FILE_NAME)
}

pub fn load_prompt_queue(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    with_queue_lock(attempt_dir, || load_and_reconcile_unlocked(attempt_dir))
}

pub fn enqueue_prompt(
    attempt_dir: &Utf8Path,
    input: impl Into<ConversationPromptInput>,
    attachment_paths: Vec<String>,
) -> Result<QueuedPrompt, PromptQueueError> {
    let input = input.into();
    if input.display_text.trim().is_empty() && attachment_paths.is_empty() {
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
            content: input.display_text,
            quotes: input.quotes,
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

pub fn reorder_queued_prompts(
    attempt_dir: &Utf8Path,
    expected_revision: u64,
    ordered_item_ids: Vec<String>,
) -> Result<PromptQueue, PromptQueueError> {
    with_typed_queue_lock(attempt_dir, || {
        let mut queue =
            load_and_reconcile_unlocked(attempt_dir).map_err(|_| PromptQueueError::Storage)?;
        if queue.revision != expected_revision {
            return Err(PromptQueueError::RevisionConflict);
        }

        let current_item_ids = queue
            .items
            .iter()
            .filter(|item| item.state == QueuedPromptState::Queued)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let current_id_set = current_item_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let ordered_id_set = ordered_item_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if ordered_item_ids.len() != current_item_ids.len()
            || ordered_id_set.len() != ordered_item_ids.len()
            || ordered_id_set != current_id_set
        {
            return Err(PromptQueueError::InvalidOrder);
        }
        if ordered_item_ids == current_item_ids {
            return Ok(queue);
        }

        let mut queued_items = queue
            .items
            .iter()
            .filter(|item| item.state == QueuedPromptState::Queued)
            .cloned()
            .map(|item| (item.id.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut reordered_items = ordered_item_ids
            .into_iter()
            .map(|item_id| {
                queued_items
                    .remove(&item_id)
                    .ok_or(PromptQueueError::InvalidOrder)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter();
        for item in &mut queue.items {
            if item.state == QueuedPromptState::Queued {
                *item = reordered_items
                    .next()
                    .ok_or(PromptQueueError::InvalidOrder)?;
            }
        }
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        Ok(queue)
    })
}

pub fn delete_queued_prompt(
    attempt_dir: &Utf8Path,
    item_id: &str,
) -> Result<PromptQueue, PromptQueueError> {
    take_queued_prompt(attempt_dir, item_id).map(|(_, queue)| queue)
}

pub fn take_queued_prompt(
    attempt_dir: &Utf8Path,
    item_id: &str,
) -> Result<(QueuedPrompt, PromptQueue), PromptQueueError> {
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
        let item = queue.items.remove(index);
        persist_mutation_unlocked(attempt_dir, &mut queue)
            .map_err(|_| PromptQueueError::Storage)?;
        Ok((item, queue))
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

/// Advances the successful-reply count for one uninterrupted automatic queue drain.
///
/// The batch is process-local by design: an application restart terminates the active
/// provider turn, so no completion from the old batch can arrive afterward. Terminal
/// completions remove the entry immediately to keep long-running desktop sessions bounded.
pub fn record_auto_dispatch_reply_completion(
    attempt_dir: &Utf8Path,
    continues: bool,
) -> Result<u32> {
    let key = queue_path(attempt_dir).to_string();
    let mut batches = AUTO_DISPATCH_REPLY_BATCHES
        .lock()
        .map_err(|_| anyhow!("prompt queue reply batch registry poisoned"))?;
    let completed_reply_count = batches
        .get(&key)
        .copied()
        .unwrap_or_default()
        .saturating_add(1);
    if continues {
        batches.insert(key, completed_reply_count);
    } else {
        batches.remove(&key);
    }
    Ok(completed_reply_count)
}

pub fn clear_auto_dispatch_reply_batch(attempt_dir: &Utf8Path) -> Result<()> {
    AUTO_DISPATCH_REPLY_BATCHES
        .lock()
        .map_err(|_| anyhow!("prompt queue reply batch registry poisoned"))?
        .remove(&queue_path(attempt_dir).to_string());
    Ok(())
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
        mark_dispatch_active(attempt_dir, &claimed)?;
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
        mark_dispatch_active(attempt_dir, &claimed).map_err(|_| PromptQueueError::Storage)?;
        Ok(claimed)
    })
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

/// Recovers one claimed item after its previous turn was found terminal.
///
/// This is deliberately scoped to `item_id`: settling every dispatch here
/// could release another in-flight prompt before its canonical acceptance
/// event is committed. A terminal turn with no canonical user-prompt event
/// was never accepted, so it is assigned a new turn ID before being retried.
pub fn recover_terminal_dispatch(
    attempt_dir: &Utf8Path,
    item_id: &str,
) -> Result<TerminalDispatchRecovery> {
    let recovery = with_queue_lock(attempt_dir, || {
        let mut queue = load_queue_unlocked(attempt_dir)?;
        let Some(index) = queue.items.iter().position(|item| item.id == item_id) else {
            return Ok(TerminalDispatchRecovery::Missing);
        };
        if queue.items[index].state != QueuedPromptState::Dispatching {
            return Ok(TerminalDispatchRecovery::Missing);
        }
        if accepted_prompt_ids(attempt_dir).contains(&queue.items[index].prompt_id) {
            queue.items.remove(index);
            persist_mutation_unlocked(attempt_dir, &mut queue)?;
            return Ok(TerminalDispatchRecovery::AlreadyAccepted);
        }

        let item = &mut queue.items[index];
        item.prompt_id = format!("turn-{}", Uuid::new_v4().simple());
        let reclaimed = item.clone();
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(TerminalDispatchRecovery::Reclaimed(reclaimed))
    })?;

    clear_dispatch_active(attempt_dir, item_id)?;
    if let TerminalDispatchRecovery::Reclaimed(item) = &recovery {
        mark_dispatch_active(attempt_dir, item)?;
    }
    Ok(recovery)
}

/// Settles every in-process dispatch against the durable timeline.
///
/// A prompt already written to the canonical timeline has left the queue even
/// if its provider turn later fails or is cancelled. Only dispatches that were
/// never accepted are restored for a future send.
pub fn settle_dispatching_prompts(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let result = with_queue_lock(attempt_dir, || {
        let mut queue = load_queue_unlocked(attempt_dir)?;
        let needs_version_reconciliation = queue.version < PROMPT_QUEUE_VERSION;
        let settled_ids = queue
            .items
            .iter()
            .filter(|item| item.state == QueuedPromptState::Dispatching)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if settled_ids.is_empty() && !needs_version_reconciliation {
            return Ok((queue, settled_ids));
        }
        let accepted_prompt_ids = accepted_prompt_ids(attempt_dir);
        queue.items.retain_mut(|item| {
            if item.state != QueuedPromptState::Dispatching {
                if needs_version_reconciliation && accepted_prompt_ids.contains(&item.prompt_id) {
                    return false;
                }
                return true;
            }
            if accepted_prompt_ids.contains(&item.prompt_id) {
                return false;
            }
            item.state = QueuedPromptState::Queued;
            true
        });
        queue.version = PROMPT_QUEUE_VERSION;
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok((queue, settled_ids))
    });
    let (queue, settled_ids) = result?;
    for item_id in settled_ids {
        clear_dispatch_active(attempt_dir, &item_id)?;
    }
    Ok(queue)
}

/// Settles only the dispatch that owns `prompt_id`.
///
/// Runtime completion is scoped to the provider turn that just finished. A
/// broad reconciliation here could release a different prompt that is still
/// between admission and its canonical acceptance event.
pub fn settle_dispatching_prompt(attempt_dir: &Utf8Path, prompt_id: &str) -> Result<bool> {
    let Some(item_id) = ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .get(&dispatch_prompt_key(attempt_dir, prompt_id))
        .map(|dispatch| dispatch.item_id.clone())
    else {
        return Ok(false);
    };
    let result = with_queue_lock(attempt_dir, || {
        if !dispatch_prompt_is_active(attempt_dir, prompt_id) {
            return Ok(false);
        }
        let mut queue = load_queue_unlocked(attempt_dir)?;
        let Some(index) = queue.items.iter().position(|item| {
            item.id == item_id
                && item.prompt_id == prompt_id
                && item.state == QueuedPromptState::Dispatching
        }) else {
            return Ok(false);
        };
        let accepted = accepted_prompt_ids(attempt_dir).contains(prompt_id);
        if accepted {
            queue.items.remove(index);
        } else {
            queue.items[index].state = QueuedPromptState::Queued;
        }
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
        Ok(true)
    });
    let settled = result?;
    if settled {
        clear_dispatch_active(attempt_dir, &item_id)?;
    }
    Ok(settled)
}

/// Removes one active queue dispatch at the durable acceptance boundary.
///
/// The in-memory lookup is the hot-path guard: ordinary prompts return without
/// reading either the queue or timeline files.
pub fn complete_accepted_prompt(attempt_dir: &Utf8Path, prompt_id: &str) -> Result<bool> {
    if !dispatch_prompt_is_active(attempt_dir, prompt_id) {
        return Ok(false);
    }
    let result = with_queue_lock(attempt_dir, || {
        if !dispatch_prompt_is_active(attempt_dir, prompt_id) {
            return Ok(false);
        }
        let mut queue = load_queue_unlocked(attempt_dir)?;
        let before = queue.items.len();
        queue.items.retain(|item| {
            item.prompt_id != prompt_id || item.state != QueuedPromptState::Dispatching
        });
        let completed = queue.items.len() != before;
        if completed {
            persist_mutation_unlocked(attempt_dir, &mut queue)?;
        }
        Ok(completed)
    });
    let completed = result?;
    clear_dispatch_active_prompt(attempt_dir, prompt_id)?;
    Ok(completed)
}

fn load_and_reconcile_unlocked(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let mut queue = load_queue_unlocked(attempt_dir)?;
    let needs_version_reconciliation = queue.version < PROMPT_QUEUE_VERSION;
    let has_orphaned_dispatch = queue.items.iter().any(|item| {
        item.state == QueuedPromptState::Dispatching && !dispatch_is_active(attempt_dir, item)
    });
    if !needs_version_reconciliation && !has_orphaned_dispatch {
        return Ok(queue);
    }
    let accepted_prompt_ids = accepted_prompt_ids(attempt_dir);
    let mut changed = needs_version_reconciliation;
    if needs_version_reconciliation {
        for item in &mut queue.items {
            item.quotes.retain(|quote| !quote.text.trim().is_empty());
        }
    }
    queue.items.retain_mut(|item| {
        let accepted = accepted_prompt_ids.contains(&item.prompt_id);
        if accepted && item.state != QueuedPromptState::Dispatching {
            changed = true;
            return false;
        }
        if item.state != QueuedPromptState::Dispatching {
            return true;
        }
        if dispatch_is_active(attempt_dir, item) {
            return true;
        }
        changed = true;
        if accepted {
            false
        } else {
            item.state = QueuedPromptState::Queued;
            true
        }
    });
    queue.version = PROMPT_QUEUE_VERSION;
    if changed {
        persist_mutation_unlocked(attempt_dir, &mut queue)?;
    }
    Ok(queue)
}

fn load_queue_unlocked(attempt_dir: &Utf8Path) -> Result<PromptQueue> {
    let path = queue_path(attempt_dir);
    if path.exists() {
        read_json::<PromptQueue>(&path)
    } else {
        Ok(PromptQueue::default())
    }
}

fn accepted_prompt_ids(attempt_dir: &Utf8Path) -> HashSet<String> {
    let timeline_path = attempt_dir.join("acp.timeline.jsonl");
    if !timeline_path.exists() {
        return HashSet::new();
    }
    crate::acp::timeline::read_indexed_accepted_prompt_ids(&timeline_path).unwrap_or_default()
}

fn mark_dispatch_active(attempt_dir: &Utf8Path, item: &QueuedPrompt) -> Result<()> {
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .insert(
            dispatch_prompt_key(attempt_dir, &item.prompt_id),
            ActiveDispatch {
                queue_path: queue_path(attempt_dir).to_string(),
                item_id: item.id.clone(),
            },
        );
    Ok(())
}

fn clear_dispatch_active(attempt_dir: &Utf8Path, item_id: &str) -> Result<()> {
    let queue_path = queue_path(attempt_dir).to_string();
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .retain(|_, dispatch| dispatch.queue_path != queue_path || dispatch.item_id != item_id);
    Ok(())
}

fn clear_dispatch_active_prompt(attempt_dir: &Utf8Path, prompt_id: &str) -> Result<()> {
    ACTIVE_DISPATCHES
        .lock()
        .map_err(|_| anyhow!("active prompt dispatch registry poisoned"))?
        .remove(&dispatch_prompt_key(attempt_dir, prompt_id));
    Ok(())
}

fn dispatch_prompt_is_active(attempt_dir: &Utf8Path, prompt_id: &str) -> bool {
    ACTIVE_DISPATCHES.lock().is_ok_and(|dispatches| {
        dispatches.contains_key(&dispatch_prompt_key(attempt_dir, prompt_id))
    })
}

fn dispatch_is_active(attempt_dir: &Utf8Path, item: &QueuedPrompt) -> bool {
    ACTIVE_DISPATCHES.lock().is_ok_and(|dispatches| {
        dispatches
            .get(&dispatch_prompt_key(attempt_dir, &item.prompt_id))
            .is_some_and(|dispatch| dispatch.item_id == item.id)
    })
}

fn dispatch_prompt_key(attempt_dir: &Utf8Path, prompt_id: &str) -> String {
    format!("{}::{prompt_id}", queue_path(attempt_dir))
}

fn persist_mutation_unlocked(attempt_dir: &Utf8Path, queue: &mut PromptQueue) -> Result<()> {
    queue.version = PROMPT_QUEUE_VERSION;
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
    use crate::acp::events::{append_timeline_patch, user_prompt_event};
    use crate::provider::conversation_prompt_text;

    fn attempt_dir(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().join("attempt-001")).unwrap()
    }

    #[test]
    fn queue_limits_capacity() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        for index in 0..MAX_QUEUED_PROMPTS {
            enqueue_prompt(&dir, format!("item-{index}"), Vec::new()).unwrap();
        }
        assert_eq!(
            enqueue_prompt(&dir, "overflow".to_string(), Vec::new()),
            Err(PromptQueueError::Full)
        );
    }

    #[test]
    fn queue_accepts_attachment_only_payloads_and_rejects_a_fully_empty_payload() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let queued = enqueue_prompt(
            &dir,
            ConversationPromptInput {
                display_text: String::new(),
                quotes: Vec::new(),
            },
            vec!["C:/temp/context.txt".to_string()],
        )
        .unwrap();

        assert!(queued.content.is_empty());
        assert_eq!(queued.attachment_paths, vec!["C:/temp/context.txt"]);
        assert_eq!(
            enqueue_prompt(&dir, String::new(), Vec::new()),
            Err(PromptQueueError::Empty)
        );
    }

    #[test]
    fn queue_preserves_structured_quotes_until_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let queued = enqueue_prompt(
            &dir,
            ConversationPromptInput {
                display_text: "继续解释".to_string(),
                quotes: vec![UserPromptQuote {
                    id: "quote-1".to_string(),
                    source_message_key: "message-1".to_string(),
                    text: "Agent 原文".to_string(),
                }],
            },
            Vec::new(),
        )
        .unwrap();

        let claimed = claim_queued_prompt(&dir, &queued.id).unwrap();
        assert_eq!(claimed.content, "继续解释");
        assert!(
            conversation_prompt_text(&claimed.content, &claimed.quotes).starts_with("> Agent 原文")
        );
        assert_eq!(claimed.quotes[0].source_message_key, "message-1");
    }

    #[test]
    fn reorder_queued_prompts_persists_the_requested_order_and_preserves_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(
            &dir,
            ConversationPromptInput {
                display_text: "first".to_string(),
                quotes: vec![UserPromptQuote {
                    id: "quote-1".to_string(),
                    source_message_key: "textDelta-message-1".to_string(),
                    text: "Agent 原文".to_string(),
                }],
            },
            Vec::new(),
        )
        .unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let third =
            enqueue_prompt(&dir, "third".to_string(), vec!["C:/third.png".to_string()]).unwrap();
        let revision = load_prompt_queue(&dir).unwrap().revision;

        let queue = reorder_queued_prompts(
            &dir,
            revision,
            vec![third.id.clone(), first.id.clone(), second.id.clone()],
        )
        .unwrap();

        assert_eq!(
            queue
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec![third.id.as_str(), first.id.as_str(), second.id.as_str()]
        );
        assert_eq!(queue.items[0].attachment_paths, vec!["C:/third.png"]);
        assert_eq!(queue.items[1].quotes, first.quotes);
        assert_eq!(queue.revision, revision + 1);
        assert_eq!(load_prompt_queue(&dir).unwrap(), queue);
    }

    #[test]
    fn reorder_queued_prompts_rejects_stale_or_invalid_orders_without_mutating_the_queue() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(&dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let original = load_prompt_queue(&dir).unwrap();

        assert_eq!(
            reorder_queued_prompts(
                &dir,
                original.revision.saturating_sub(1),
                vec![second.id.clone(), first.id.clone()],
            ),
            Err(PromptQueueError::RevisionConflict)
        );
        assert_eq!(load_prompt_queue(&dir).unwrap(), original);

        assert_eq!(
            reorder_queued_prompts(
                &dir,
                original.revision,
                vec![first.id.clone(), first.id.clone()],
            ),
            Err(PromptQueueError::InvalidOrder)
        );
        assert_eq!(load_prompt_queue(&dir).unwrap(), original);

        let unchanged = reorder_queued_prompts(
            &dir,
            original.revision,
            vec![first.id.clone(), second.id.clone()],
        )
        .unwrap();
        assert_eq!(unchanged.revision, original.revision);
        assert_eq!(unchanged, original);
    }

    #[test]
    fn taking_a_queued_prompt_returns_its_complete_authoring_payload_and_removes_it() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let queued = enqueue_prompt(
            &dir,
            ConversationPromptInput {
                display_text: "restore me".to_string(),
                quotes: vec![UserPromptQuote {
                    id: "quote-1".to_string(),
                    source_message_key: "message-1".to_string(),
                    text: "quoted".to_string(),
                }],
            },
            vec!["C:/evidence.png".to_string()],
        )
        .unwrap();

        let (restored, queue) = take_queued_prompt(&dir, &queued.id).unwrap();

        assert_eq!(restored, queued);
        assert!(queue.items.is_empty());
        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
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
    fn automatic_reply_batch_counts_until_terminal_completion_then_resets() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);

        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, true).unwrap(),
            1
        );
        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, true).unwrap(),
            2
        );
        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, false).unwrap(),
            3
        );
        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, false).unwrap(),
            1
        );
    }

    #[test]
    fn clearing_automatic_reply_batch_starts_the_next_batch_from_one() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);

        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, true).unwrap(),
            1
        );
        clear_auto_dispatch_reply_batch(&dir).unwrap();
        assert_eq!(
            record_auto_dispatch_reply_completion(&dir, false).unwrap(),
            1
        );
    }

    #[test]
    fn accepted_manual_use_removes_selected_item_without_reordering_remaining_items() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(&dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let third = enqueue_prompt(&dir, "third".to_string(), Vec::new()).unwrap();
        let claimed = claim_queued_prompt(&dir, &second.id).unwrap();
        assert_eq!(claimed.id, second.id);
        assert!(complete_accepted_prompt(&dir, &claimed.prompt_id).unwrap());
        let queue = load_prompt_queue(&dir).unwrap();
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
    fn ordinary_prompt_acceptance_does_not_touch_queue_storage() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);

        assert!(!complete_accepted_prompt(&dir, "ordinary-direct-prompt").unwrap());
        assert!(!queue_path(&dir).exists());
    }

    #[test]
    fn accepted_prompt_id_only_completes_its_active_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "queued".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&dir, &item.id).unwrap();

        assert!(!complete_accepted_prompt(&dir, "different-prompt").unwrap());
        assert_eq!(
            load_prompt_queue(&dir).unwrap().items[0].state,
            QueuedPromptState::Dispatching
        );
        assert!(complete_accepted_prompt(&dir, &item.prompt_id).unwrap());
        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
    }

    #[test]
    fn runtime_settlement_only_releases_the_matching_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let first = enqueue_prompt(&dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&dir, "second".to_string(), Vec::new()).unwrap();
        let first = claim_queued_prompt(&dir, &first.id).unwrap();
        let second = claim_queued_prompt(&dir, &second.id).unwrap();

        assert!(settle_dispatching_prompt(&dir, &first.prompt_id).unwrap());
        let queue = load_prompt_queue(&dir).unwrap();
        assert_eq!(
            queue
                .items
                .iter()
                .find(|item| item.id == first.id)
                .unwrap()
                .state,
            QueuedPromptState::Queued
        );
        assert_eq!(
            queue
                .items
                .iter()
                .find(|item| item.id == second.id)
                .unwrap()
                .state,
            QueuedPromptState::Dispatching
        );
        assert!(settle_dispatching_prompt(&dir, &second.prompt_id).unwrap());
    }

    #[test]
    fn active_prompt_identity_is_scoped_to_its_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let first_dir = Utf8PathBuf::from_path_buf(temp.path().join("attempt-001")).unwrap();
        let second_dir = Utf8PathBuf::from_path_buf(temp.path().join("attempt-002")).unwrap();
        let first = enqueue_prompt(&first_dir, "first".to_string(), Vec::new()).unwrap();
        let second = enqueue_prompt(&second_dir, "second".to_string(), Vec::new()).unwrap();
        let mut second_queue = load_prompt_queue(&second_dir).unwrap();
        second_queue.items[0].prompt_id = first.prompt_id.clone();
        write_json(&queue_path(&second_dir), &second_queue).unwrap();
        claim_queued_prompt(&first_dir, &first.id).unwrap();
        claim_queued_prompt(&second_dir, &second.id).unwrap();

        assert!(complete_accepted_prompt(&first_dir, &first.prompt_id).unwrap());
        assert!(load_prompt_queue(&first_dir).unwrap().items.is_empty());
        assert_eq!(load_prompt_queue(&second_dir).unwrap().items.len(), 1);
        assert!(complete_accepted_prompt(&second_dir, &first.prompt_id).unwrap());
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
    fn cancelled_turn_does_not_restore_a_durably_accepted_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "accepted".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&dir, &item.id).unwrap();
        let event = user_prompt_event(
            1,
            "session-001".to_string(),
            item.content.clone(),
            Some(item.prompt_id.clone()),
            false,
            Vec::new(),
        );
        append_timeline_patch(
            &dir.join("acp.timeline.jsonl"),
            event.id.clone(),
            event.seq,
            &event,
        )
        .unwrap();

        let queue = settle_dispatching_prompts(&dir).unwrap();

        assert!(queue.items.is_empty());
        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
        assert_eq!(
            delete_queued_prompt(&dir, &item.id),
            Err(PromptQueueError::NotFound)
        );
    }

    #[test]
    fn failure_before_durable_acceptance_restores_the_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "not accepted".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&dir, &item.id).unwrap();

        let queue = settle_dispatching_prompts(&dir).unwrap();

        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].id, item.id);
        assert_eq!(queue.items[0].state, QueuedPromptState::Queued);
    }

    #[test]
    fn terminal_unaccepted_dispatch_receives_a_fresh_turn_identity() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item =
            enqueue_prompt(&dir, "retry after interruption".to_string(), Vec::new()).unwrap();
        let claimed = claim_queued_prompt(&dir, &item.id).unwrap();

        let TerminalDispatchRecovery::Reclaimed(reclaimed) =
            recover_terminal_dispatch(&dir, &claimed.id).unwrap()
        else {
            panic!("unaccepted terminal dispatch must be reclaimed");
        };
        assert_eq!(reclaimed.id, claimed.id);
        assert_ne!(reclaimed.prompt_id, claimed.prompt_id);
        assert_eq!(reclaimed.state, QueuedPromptState::Dispatching);
        assert!(complete_accepted_prompt(&dir, &reclaimed.prompt_id).unwrap());
        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
    }

    #[test]
    fn terminal_accepted_dispatch_is_removed_without_a_retry_turn() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "already accepted".to_string(), Vec::new()).unwrap();
        let claimed = claim_queued_prompt(&dir, &item.id).unwrap();
        let event = user_prompt_event(
            1,
            "session-001".to_string(),
            claimed.content.clone(),
            Some(claimed.prompt_id.clone()),
            false,
            Vec::new(),
        );
        append_timeline_patch(
            &dir.join("acp.timeline.jsonl"),
            event.id.clone(),
            event.seq,
            &event,
        )
        .unwrap();

        assert_eq!(
            recover_terminal_dispatch(&dir, &claimed.id).unwrap(),
            TerminalDispatchRecovery::AlreadyAccepted
        );
        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
    }

    #[test]
    fn legacy_queued_item_is_removed_when_its_prompt_was_already_accepted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = attempt_dir(&temp);
        let item = enqueue_prompt(&dir, "legacy duplicate".to_string(), Vec::new()).unwrap();
        let mut legacy_queue = load_prompt_queue(&dir).unwrap();
        legacy_queue.version = 1;
        write_json(&queue_path(&dir), &legacy_queue).unwrap();
        let event = user_prompt_event(
            1,
            "session-001".to_string(),
            item.content,
            Some(item.prompt_id),
            false,
            Vec::new(),
        );
        append_timeline_patch(
            &dir.join("acp.timeline.jsonl"),
            event.id.clone(),
            event.seq,
            &event,
        )
        .unwrap();

        assert!(load_prompt_queue(&dir).unwrap().items.is_empty());
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
