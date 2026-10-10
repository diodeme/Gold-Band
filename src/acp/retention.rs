//! Direct retention decisions operate on live, deduplicated provider sessions.
use super::events::AcpUiEvent;
use crate::config::DirectSessionRetentionConfig;
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
    pub policy: DirectSessionRetentionConfig,
    pub live_update: Option<SessionLiveUpdate>,
}

pub(crate) struct RetentionCandidate<K> {
    pub key: K,
    pub last_activity: Instant,
    pub protected: bool,
}

pub(crate) fn eviction_candidates<K: Clone>(
    policy: DirectSessionRetentionConfig,
    now: Instant,
    mut sessions: Vec<RetentionCandidate<K>>,
) -> Vec<K> {
    let mut count = sessions.len();
    sessions.sort_by_key(|entry| entry.last_activity);
    let mut evicted = Vec::new();
    for entry in sessions {
        if entry.protected || count <= policy.resident_threshold {
            continue;
        }
        if count > policy.capacity_target
            || now.saturating_duration_since(entry.last_activity)
                >= Duration::from_secs(policy.idle_ttl_secs)
        {
            evicted.push(entry.key);
            count -= 1;
        }
    }
    evicted
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidates(count: usize, age: Duration, protected: bool) -> Vec<RetentionCandidate<usize>> {
        (0..count)
            .map(|key| RetentionCandidate {
                key,
                last_activity: Instant::now() - age,
                protected,
            })
            .collect()
    }
    #[test]
    fn direct_capacity_and_ttl_boundaries() {
        let policy = DirectSessionRetentionConfig::default();
        for (count, age, expected) in [
            (8, 21601, 0),
            (9, 21599, 0),
            (9, 21601, 1),
            (20, 10, 0),
            (21, 10, 1),
            (21, 21601, 13),
        ] {
            let entries = candidates(count, Duration::from_secs(age), false);
            assert_eq!(
                eviction_candidates(policy, Instant::now(), entries).len(),
                expected
            );
        }
    }
    #[test]
    fn protected_sessions_can_exceed_capacity() {
        let entries = candidates(25, Duration::from_secs(30000), true);
        assert!(
            eviction_candidates(
                DirectSessionRetentionConfig::default(),
                Instant::now(),
                entries
            )
            .is_empty()
        );
    }
    #[test]
    fn fresh_activity_renews_the_idle_window() {
        let mut entries = candidates(9, Duration::from_secs(21601), false);
        entries[0].last_activity = Instant::now();
        let evicted = eviction_candidates(
            DirectSessionRetentionConfig::default(),
            Instant::now(),
            entries,
        );
        assert_eq!(evicted.len(), 1);
        assert_ne!(evicted[0], 0);
    }
    #[test]
    fn invalid_configuration_is_rejected() {
        let mut policy = DirectSessionRetentionConfig::default();
        policy.capacity_target = policy.resident_threshold;
        assert!(policy.validate().is_err());
        policy = DirectSessionRetentionConfig::default();
        policy.idle_ttl_secs = 0;
        assert!(policy.validate().is_err());
    }
}
