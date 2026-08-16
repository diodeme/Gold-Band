use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::future::poll_fn;
use std::time::Duration;
use tokio_util::time::{DelayQueue, delay_queue};

/// Identifies one persisted scheduled definition across all registered workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScheduledJobKey {
    pub workspace_path: Utf8PathBuf,
    pub project_id: String,
    pub job_id: String,
}

impl ScheduledJobKey {
    pub fn new(
        workspace_path: Utf8PathBuf,
        project_id: impl Into<String>,
        job_id: impl Into<String>,
    ) -> Self {
        Self {
            workspace_path,
            project_id: project_id.into(),
            job_id: job_id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileReason {
    Startup,
    SystemResume,
    TimerDrift,
    Explicit,
}

/// Owns the process-local timer registrations. SQLite remains the authority for
/// job state; this registry only wakes the runtime at the next known deadline.
pub struct DeadlineRegistry {
    queue: DelayQueue<ScheduledJobKey>,
    entries: HashMap<ScheduledJobKey, delay_queue::Key>,
}

impl Default for DeadlineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DeadlineRegistry {
    pub fn new() -> Self {
        Self {
            queue: DelayQueue::new(),
            entries: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, key: &ScheduledJobKey) -> bool {
        self.entries.contains_key(key)
    }

    pub fn register_at(
        &mut self,
        key: ScheduledJobKey,
        deadline: DateTime<Utc>,
        now: DateTime<Utc>,
    ) {
        let delay = deadline
            .signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO);
        self.register_after(key, delay);
    }

    pub fn register_after(&mut self, key: ScheduledJobKey, delay: Duration) {
        if let Some(previous) = self.entries.remove(&key) {
            self.queue.remove(&previous);
        }
        let entry = self.queue.insert(key.clone(), delay);
        self.entries.insert(key, entry);
    }

    pub fn cancel(&mut self, key: &ScheduledJobKey) -> bool {
        let Some(entry) = self.entries.remove(key) else {
            return false;
        };
        self.queue.remove(&entry);
        true
    }

    pub async fn next_expired(&mut self) -> Option<ScheduledJobKey> {
        let expired = poll_fn(|context| self.queue.poll_expired(context)).await?;
        let key = expired.into_inner();
        self.entries.remove(&key);
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::{DeadlineRegistry, ScheduledJobKey};
    use camino::Utf8PathBuf;
    use chrono::{Duration as ChronoDuration, Utc};
    use std::time::Duration;

    fn key(job_id: &str) -> ScheduledJobKey {
        ScheduledJobKey::new(Utf8PathBuf::from("C:/workspace"), "project-a", job_id)
    }

    #[tokio::test(start_paused = true)]
    async fn create_registers_exactly_one_future_deadline() {
        let mut registry = DeadlineRegistry::new();
        let now = Utc::now();
        registry.register_at(key("job-a"), now + ChronoDuration::seconds(60), now);

        assert_eq!(registry.len(), 1);
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(
            tokio::time::timeout(Duration::ZERO, registry.next_expired())
                .await
                .is_err()
        );
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(registry.next_expired().await, Some(key("job-a")));
        assert!(registry.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn update_replaces_stale_deadline() {
        let mut registry = DeadlineRegistry::new();
        let now = Utc::now();
        registry.register_at(key("job-a"), now + ChronoDuration::seconds(60), now);
        registry.register_at(key("job-a"), now + ChronoDuration::seconds(120), now);

        assert_eq!(registry.len(), 1);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(
            tokio::time::timeout(Duration::ZERO, registry.next_expired())
                .await
                .is_err()
        );
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(registry.next_expired().await, Some(key("job-a")));
    }

    #[tokio::test(start_paused = true)]
    async fn disable_and_delete_cancel_deadline() {
        let mut registry = DeadlineRegistry::new();
        registry.register_after(key("disabled"), Duration::from_secs(60));
        registry.register_after(key("deleted"), Duration::from_secs(60));

        assert!(registry.cancel(&key("disabled")));
        assert!(registry.cancel(&key("deleted")));
        assert!(registry.is_empty());
        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(registry.next_expired().await, None);
    }
}
