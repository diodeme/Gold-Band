use super::OverlapPolicy;
use chrono::{DateTime, Duration, Utc};

pub const QUEUE_RETRY_INTERVAL: Duration = Duration::seconds(30);
pub const QUEUE_MAX_RETRIES: u8 = 3;
pub const LATE_FIRE_GRACE: Duration = Duration::seconds(60);
pub const DEFAULT_OCCURRENCE_RETENTION_DAYS: u16 = 30;
pub const MIN_OCCURRENCE_RETENTION_DAYS: u16 = 1;
pub const MAX_OCCURRENCE_RETENTION_DAYS: u16 = 3650;
pub const RETENTION_DELETE_BATCH_SIZE: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveExecution {
    Idle,
    Running,
    PermissionWaiting,
    WaitingForUserInput,
    ResumablePaused,
}

impl ActiveExecution {
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDecision {
    StartNow,
    Skipped,
    RetryAt(DateTime<Utc>),
}

pub fn decide_queue(
    policy: OverlapPolicy,
    active: ActiveExecution,
    retry_count: u8,
    now: DateTime<Utc>,
) -> QueueDecision {
    if !active.is_active() {
        return QueueDecision::StartNow;
    }
    match policy {
        OverlapPolicy::SkipWhenRunning => QueueDecision::Skipped,
        OverlapPolicy::RetryWhenBusy if retry_count < QUEUE_MAX_RETRIES => {
            QueueDecision::RetryAt(now + QUEUE_RETRY_INTERVAL)
        }
        OverlapPolicy::RetryWhenBusy => QueueDecision::Skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveExecution, DEFAULT_OCCURRENCE_RETENTION_DAYS, LATE_FIRE_GRACE,
        MAX_OCCURRENCE_RETENTION_DAYS, MIN_OCCURRENCE_RETENTION_DAYS, QUEUE_MAX_RETRIES,
        QUEUE_RETRY_INTERVAL, QueueDecision, RETENTION_DELETE_BATCH_SIZE, decide_queue,
    };
    use crate::scheduler::OverlapPolicy;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn inactive_task_can_start_immediately_for_both_policies() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 10, 0, 0).unwrap();

        assert_eq!(
            decide_queue(
                OverlapPolicy::SkipWhenRunning,
                ActiveExecution::Idle,
                0,
                now
            ),
            QueueDecision::StartNow
        );
        assert_eq!(
            decide_queue(OverlapPolicy::RetryWhenBusy, ActiveExecution::Idle, 0, now),
            QueueDecision::StartNow
        );
    }

    #[test]
    fn skip_policy_marks_an_active_occurrence_skipped() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 10, 0, 0).unwrap();

        assert_eq!(
            decide_queue(
                OverlapPolicy::SkipWhenRunning,
                ActiveExecution::PermissionWaiting,
                0,
                now,
            ),
            QueueDecision::Skipped
        );
    }

    #[test]
    fn retry_policy_retries_every_thirty_seconds_then_skips_after_three_retries() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 10, 0, 0).unwrap();

        assert_eq!(
            decide_queue(
                OverlapPolicy::RetryWhenBusy,
                ActiveExecution::WaitingForUserInput,
                2,
                now,
            ),
            QueueDecision::RetryAt(now + Duration::seconds(30))
        );
        assert_eq!(
            decide_queue(
                OverlapPolicy::RetryWhenBusy,
                ActiveExecution::ResumablePaused,
                3,
                now,
            ),
            QueueDecision::Skipped
        );
    }

    #[test]
    fn queue_and_retention_policy_constants_have_stable_boundaries() {
        assert_eq!(QUEUE_RETRY_INTERVAL, Duration::seconds(30));
        assert_eq!(QUEUE_MAX_RETRIES, 3);
        assert_eq!(LATE_FIRE_GRACE, Duration::seconds(60));
        assert_eq!(DEFAULT_OCCURRENCE_RETENTION_DAYS, 30);
        assert_eq!(MIN_OCCURRENCE_RETENTION_DAYS, 1);
        assert_eq!(MAX_OCCURRENCE_RETENTION_DAYS, 3650);
        assert_eq!(RETENTION_DELETE_BATCH_SIZE, 500);
    }

    #[test]
    fn all_runtime_waiting_states_are_active() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 10, 0, 0).unwrap();
        for state in [
            ActiveExecution::Running,
            ActiveExecution::PermissionWaiting,
            ActiveExecution::WaitingForUserInput,
            ActiveExecution::ResumablePaused,
        ] {
            assert_ne!(
                decide_queue(OverlapPolicy::SkipWhenRunning, state, 0, now),
                QueueDecision::StartNow
            );
        }
    }
}
