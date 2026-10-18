//! One idle-retention decision for every attached ACP session runtime.
//!
//! Direct and Workflow sessions share the idle TTL and idle cap. Mode-specific
//! facts (a running turn, a foreground lease, a resident Direct conversation,
//! in-flight Direct tools) only decide whether a session is protected.
use super::events::AcpUiEvent;
use crate::provider::AcpLiveTimelinePosition;
use anyhow::Result;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub type SessionLiveUpdate =
    Arc<dyn Fn(&AcpUiEvent, AcpLiveTimelinePosition) -> Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct DirectSessionRegistration {
    pub project_id: String,
    pub task_id: String,
    pub resident: bool,
    pub background_stop_grace: Duration,
    pub live_update: Option<SessionLiveUpdate>,
}

pub(crate) struct RetentionCandidate<K> {
    pub key: K,
    pub last_activity: Instant,
    pub protected: bool,
}

/// Unprotected sessions are idle. An idle session is evicted once it has been
/// idle for `idle_ttl`, and the oldest idle sessions are evicted while more
/// than `max_idle` remain. Protected sessions never count toward the cap.
pub(crate) fn eviction_candidates<K>(
    idle_ttl: Duration,
    max_idle: usize,
    now: Instant,
    sessions: Vec<RetentionCandidate<K>>,
) -> Vec<K> {
    let mut idle = sessions
        .into_iter()
        .filter(|entry| !entry.protected)
        .collect::<Vec<_>>();
    idle.sort_by_key(|entry| entry.last_activity);
    let overflow = idle.len().saturating_sub(max_idle);
    idle.into_iter()
        .enumerate()
        .filter(|(index, entry)| {
            *index < overflow || now.saturating_duration_since(entry.last_activity) >= idle_ttl
        })
        .map(|(_, entry)| entry.key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(600);
    const MAX_IDLE: usize = 8;

    fn candidates(count: usize, age: Duration, protected: bool) -> Vec<RetentionCandidate<usize>> {
        let now = Instant::now();
        (0..count)
            .map(|key| RetentionCandidate {
                key,
                last_activity: now - age,
                protected,
            })
            .collect()
    }

    #[test]
    fn idle_ttl_and_cap_boundaries() {
        for (count, age, expected) in [
            (1, 599, 0),
            (1, 601, 1),
            (8, 10, 0),
            (9, 10, 1),
            (12, 10, 4),
            (12, 601, 12),
        ] {
            let entries = candidates(count, Duration::from_secs(age), false);
            assert_eq!(
                eviction_candidates(TTL, MAX_IDLE, Instant::now(), entries).len(),
                expected,
                "count={count} age={age}"
            );
        }
    }

    #[test]
    fn protected_sessions_neither_expire_nor_count_toward_the_cap() {
        let mut entries = candidates(20, Duration::from_secs(30_000), true);
        assert!(eviction_candidates(TTL, MAX_IDLE, Instant::now(), entries).is_empty());
        entries = candidates(20, Duration::from_secs(10), true);
        entries.extend(
            candidates(8, Duration::from_secs(10), false)
                .into_iter()
                .map(|mut entry| {
                    entry.key += 100;
                    entry
                }),
        );
        assert!(eviction_candidates(TTL, MAX_IDLE, Instant::now(), entries).is_empty());
    }

    #[test]
    fn cap_overflow_evicts_the_least_recently_active_sessions() {
        let now = Instant::now();
        let entries = (0..10)
            .map(|key| RetentionCandidate {
                key,
                last_activity: now - Duration::from_secs(100 - key as u64),
                protected: false,
            })
            .collect();
        let mut evicted = eviction_candidates(TTL, MAX_IDLE, now, entries);
        evicted.sort_unstable();
        assert_eq!(evicted, vec![0, 1]);
    }

    #[test]
    fn fresh_activity_renews_the_idle_window() {
        let mut entries = candidates(3, Duration::from_secs(601), false);
        entries[0].last_activity = Instant::now();
        let evicted = eviction_candidates(TTL, MAX_IDLE, Instant::now(), entries);
        assert_eq!(evicted.len(), 2);
        assert!(!evicted.contains(&0));
    }
}
