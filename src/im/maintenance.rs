use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::{ImRepository, ImRepositoryError, MAX_IM_CLEANUP_BATCH};

pub const IM_LEASE_RECOVERY_INTERVAL: Duration = Duration::from_secs(30);
pub const IM_RETENTION_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
pub const IM_OUTBOX_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const IM_INBOUND_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const MAX_IM_MAINTENANCE_BATCHES_PER_PASS: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImMaintenanceResult {
    pub recovered_leases: usize,
    pub removed_outbox: usize,
    pub removed_inbound: usize,
}

pub struct ImMaintenanceWorker {
    repository: Arc<ImRepository>,
}

impl ImMaintenanceWorker {
    pub fn new(repository: Arc<ImRepository>) -> Self {
        Self { repository }
    }

    pub async fn run_startup_pass(
        &self,
        now_ms: i64,
    ) -> Result<ImMaintenanceResult, ImRepositoryError> {
        let recovered_leases = self.recover_leases(now_ms).await?;
        let (removed_outbox, removed_inbound) = self.cleanup_retained(now_ms).await?;
        Ok(ImMaintenanceResult {
            recovered_leases,
            removed_outbox,
            removed_inbound,
        })
    }

    pub async fn run(self, cancellation: CancellationToken) {
        let mut lease_interval = tokio::time::interval(IM_LEASE_RECOVERY_INTERVAL);
        lease_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut retention_interval = tokio::time::interval(IM_RETENTION_INTERVAL);
        retention_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        lease_interval.tick().await;
        retention_interval.tick().await;

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = lease_interval.tick() => {
                    if let Err(error) = self.recover_leases(now_millis()).await {
                        tracing::warn!(error_code = error.code(), "IM lease recovery failed");
                    }
                }
                _ = retention_interval.tick() => {
                    if let Err(error) = self.cleanup_retained(now_millis()).await {
                        tracing::warn!(error_code = error.code(), "IM retention cleanup failed");
                    }
                }
            }
        }
    }

    async fn recover_leases(&self, now_ms: i64) -> Result<usize, ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        tokio::task::spawn_blocking(move || {
            let mut total = 0usize;
            for _ in 0..MAX_IM_MAINTENANCE_BATCHES_PER_PASS {
                let changed = repository.recover_expired_leases(now_ms, MAX_IM_CLEANUP_BATCH)?;
                total = total.saturating_add(changed);
                if changed < MAX_IM_CLEANUP_BATCH {
                    break;
                }
            }
            Ok(total)
        })
        .await
        .map_err(|_| ImRepositoryError::LockUnavailable)?
    }

    async fn cleanup_retained(&self, now_ms: i64) -> Result<(usize, usize), ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        tokio::task::spawn_blocking(move || {
            let outbox_before_ms = now_ms.saturating_sub(duration_millis(IM_OUTBOX_RETENTION));
            let inbound_before_ms = now_ms.saturating_sub(duration_millis(IM_INBOUND_RETENTION));
            let mut outbox_total = 0usize;
            let mut inbound_total = 0usize;
            for _ in 0..MAX_IM_MAINTENANCE_BATCHES_PER_PASS {
                let (outbox, inbound) = repository.cleanup_retained(
                    outbox_before_ms,
                    inbound_before_ms,
                    MAX_IM_CLEANUP_BATCH,
                )?;
                outbox_total = outbox_total.saturating_add(outbox);
                inbound_total = inbound_total.saturating_add(inbound);
                if outbox < MAX_IM_CLEANUP_BATCH && inbound < MAX_IM_CLEANUP_BATCH {
                    break;
                }
            }
            Ok((outbox_total, inbound_total))
        })
        .await
        .map_err(|_| ImRepositoryError::LockUnavailable)?
    }
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;
    use crate::im::{
        IM_PAYLOAD_VERSION, ImChannelKind, ImDelivery, ImDeliveryPayload, ImDestination,
        ImNavigationLocator, ImNotificationKind, InformationalNotification,
    };

    fn delivery(event_id: &str, expires_at_ms: i64) -> ImDelivery {
        let channel = ImChannelKind::WeCom;
        let kind = ImNotificationKind::RunFailure;
        ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, "user-1", kind, event_id),
            channel,
            destination: ImDestination {
                destination_id: "user-1".into(),
                conversation_id: "chat-1".into(),
                authorized_actor_id: "actor-1".into(),
            },
            notification_kind: kind,
            canonical_event_id: event_id.into(),
            payload: ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: event_id.into(),
                    notification_kind: kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: None,
                        run_id: None,
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "failure".into(),
                    summary_key: "im.notification.runFailure.summary".into(),
                    parameters: Default::default(),
                    missed_count: None,
                },
            },
            expires_at_ms,
            display_ref: None,
        }
    }

    #[tokio::test]
    async fn startup_pass_recovers_valid_lease_and_expires_invalid_lease() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        repository.enqueue(&delivery("valid", 10_000), 100).unwrap();
        repository.enqueue(&delivery("expired", 120), 100).unwrap();
        repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_millis(50), 2)
            .unwrap();

        let result = ImMaintenanceWorker::new(Arc::clone(&repository))
            .run_startup_pass(150)
            .await
            .unwrap();
        assert_eq!(result.recovered_leases, 2);
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 150, Duration::from_secs(60), 2)
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].delivery.canonical_event_id, "valid");
    }

    #[tokio::test]
    async fn startup_pass_applies_outbox_retention_in_bounded_storage_work() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let retained = delivery("retained", i64::MAX);
        repository.enqueue(&retained, 100).unwrap();
        let claimed = repository
            .claim_due(ImChannelKind::WeCom, 100, Duration::from_secs(30), 1)
            .unwrap();
        repository
            .mark_sent(
                &retained.delivery_id,
                retained.channel,
                claimed[0].attempt_count,
                None,
                101,
            )
            .unwrap();

        let result = ImMaintenanceWorker::new(Arc::clone(&repository))
            .run_startup_pass(101 + duration_millis(IM_OUTBOX_RETENTION) + 1)
            .await
            .unwrap();

        assert_eq!(result.removed_outbox, 1);
        assert!(
            repository
                .delivery(&retained.delivery_id)
                .unwrap()
                .is_none()
        );
    }
}
