use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{
    IM_LIFECYCLE_QUEUE_CAPACITY, ImChannelKind, ImConnector, ImRepository, ImRepositoryError,
    MAX_IM_DUE_BATCH,
};

pub const DEFAULT_IM_SEND_CONCURRENCY: usize = 4;
pub const DEFAULT_IM_LEASE_DURATION: Duration = Duration::from_secs(60);
pub const DEFAULT_IM_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImWorkerSignal {
    DeliveryAvailable,
}

pub fn im_worker_channel() -> (mpsc::Sender<ImWorkerSignal>, mpsc::Receiver<ImWorkerSignal>) {
    mpsc::channel(IM_LIFECYCLE_QUEUE_CAPACITY)
}

pub struct ImDeliveryWorker {
    channel: ImChannelKind,
    repository: Arc<ImRepository>,
    connector: Arc<dyn ImConnector>,
    concurrency: usize,
    lease_duration: Duration,
}

impl ImDeliveryWorker {
    pub fn new(
        channel: ImChannelKind,
        repository: Arc<ImRepository>,
        connector: Arc<dyn ImConnector>,
    ) -> Self {
        Self {
            channel,
            repository,
            connector,
            concurrency: DEFAULT_IM_SEND_CONCURRENCY,
            lease_duration: DEFAULT_IM_LEASE_DURATION,
        }
    }

    #[cfg(test)]
    fn with_limits(mut self, concurrency: usize, lease_duration: Duration) -> Self {
        self.concurrency = concurrency.max(1);
        self.lease_duration = lease_duration;
        self
    }

    pub async fn run(
        self,
        mut signals: mpsc::Receiver<ImWorkerSignal>,
        cancellation: CancellationToken,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                signal = signals.recv() => match signal {
                    None => break,
                    Some(ImWorkerSignal::DeliveryAvailable) => {
                        if !self.dispatch_or_cancel(now_millis(), &cancellation).await {
                            break;
                        }
                    }
                },
                _ = interval.tick() => {
                    if !self.dispatch_or_cancel(now_millis(), &cancellation).await {
                        break;
                    }
                }
            }
        }
    }

    async fn dispatch_or_cancel(&self, now_ms: i64, cancellation: &CancellationToken) -> bool {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => false,
            _ = self.dispatch_once(now_ms) => true,
        }
    }

    pub async fn dispatch_once(&self, now_ms: i64) -> Result<usize, ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        let channel = self.channel;
        let lease_duration = self.lease_duration;
        let deliveries = tokio::task::spawn_blocking(move || {
            repository.claim_due(channel, now_ms, lease_duration, MAX_IM_DUE_BATCH)
        })
        .await
        .map_err(|_| ImRepositoryError::LockUnavailable)??;
        let count = deliveries.len();
        let mut pending = deliveries.into_iter();
        let mut sends = JoinSet::new();
        for _ in 0..self.concurrency {
            let Some(claimed) = pending.next() else {
                break;
            };
            spawn_send(&mut sends, Arc::clone(&self.connector), claimed);
        }
        while let Some(result) = sends.join_next().await {
            if let Ok((claimed, send_result)) = result {
                self.settle_send(claimed, send_result, now_ms).await?;
            }
            if let Some(claimed) = pending.next() {
                spawn_send(&mut sends, Arc::clone(&self.connector), claimed);
            }
        }
        Ok(count)
    }

    async fn settle_send(
        &self,
        claimed: super::ClaimedImDelivery,
        result: Result<super::ImDeliveryReceipt, super::ImIntegrationError>,
        now_ms: i64,
    ) -> Result<(), ImRepositoryError> {
        let repository = Arc::clone(&self.repository);
        let delivery = claimed.delivery;
        let expected_attempt_count = claimed.attempt_count;
        match result {
            Ok(receipt) => {
                let delivery_id = delivery.delivery_id;
                let channel = delivery.channel;
                let binding = receipt.into_binding(channel, delivery_id.clone());
                tokio::task::spawn_blocking(move || {
                    repository.mark_sent(
                        &delivery_id,
                        channel,
                        expected_attempt_count,
                        binding.as_ref(),
                        now_ms,
                    )
                })
                .await
                .map_err(|_| ImRepositoryError::LockUnavailable)??;
            }
            Err(error) => {
                let retry_at_ms = error.retryable.then(|| {
                    now_ms.saturating_add(duration_millis(
                        error.retry_after.unwrap_or(DEFAULT_IM_RETRY_DELAY),
                    ))
                });
                let delivery_id = delivery.delivery_id;
                let error_code = error.code.as_str();
                tracing::warn!(
                    channel = delivery.channel.as_str(),
                    delivery_id,
                    error_code,
                    platform_code = ?error.platform_code,
                    "IM delivery failed"
                );
                tokio::task::spawn_blocking(move || {
                    repository.mark_failed(
                        &delivery_id,
                        expected_attempt_count,
                        retry_at_ms,
                        error_code,
                        now_ms,
                    )
                })
                .await
                .map_err(|_| ImRepositoryError::LockUnavailable)??;
            }
        }
        Ok(())
    }
}

fn spawn_send(
    sends: &mut JoinSet<(
        super::ClaimedImDelivery,
        Result<super::ImDeliveryReceipt, super::ImIntegrationError>,
    )>,
    connector: Arc<dyn ImConnector>,
    claimed: super::ClaimedImDelivery,
) {
    sends.spawn(async move {
        let delivery = claimed.delivery.clone();
        let result = connector.send(delivery.clone()).await;
        (claimed, result)
    });
}

fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    use super::*;
    use crate::im::{
        IM_PAYLOAD_VERSION, ImActionResponseContext, ImChannelCapabilities, ImDelivery,
        ImDeliveryPayload, ImDeliveryReceipt, ImDestination, ImIntegrationError, ImMessageState,
        ImNavigationLocator, ImNotificationKind, InformationalNotification,
        ResolvedImChannelConfig,
    };

    struct FakeConnector {
        sends: AtomicUsize,
    }

    struct BlockingConnector {
        started: tokio::sync::Notify,
    }

    #[async_trait]
    impl ImConnector for BlockingConnector {
        fn kind(&self) -> ImChannelKind {
            ImChannelKind::WeCom
        }

        fn capabilities(&self) -> ImChannelCapabilities {
            ImChannelCapabilities::default()
        }

        fn advance_generation(&self, _generation: u64) {}

        async fn connect(
            &self,
            _config: ResolvedImChannelConfig,
            _generation: u64,
            _events: mpsc::Sender<super::super::ImConnectorEvent>,
            _cancellation: CancellationToken,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn send(
            &self,
            _delivery: ImDelivery,
        ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
            self.started.notify_one();
            pending().await
        }

        async fn respond_to_action(
            &self,
            _context: ImActionResponseContext,
            _state: ImMessageState,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ImConnector for FakeConnector {
        fn kind(&self) -> ImChannelKind {
            ImChannelKind::WeCom
        }

        fn capabilities(&self) -> ImChannelCapabilities {
            ImChannelCapabilities::default()
        }

        fn advance_generation(&self, _generation: u64) {}

        async fn connect(
            &self,
            _config: ResolvedImChannelConfig,
            _generation: u64,
            _events: mpsc::Sender<super::super::ImConnectorEvent>,
            _cancellation: CancellationToken,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }

        async fn send(
            &self,
            delivery: ImDelivery,
        ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(ImDeliveryReceipt {
                platform_message_id: Some(format!("message-{}", delivery.delivery_id)),
                platform_chat_id: delivery.destination.conversation_id,
                update_token_ref: None,
            })
        }

        async fn respond_to_action(
            &self,
            _context: ImActionResponseContext,
            _state: ImMessageState,
        ) -> Result<(), ImIntegrationError> {
            Ok(())
        }
    }

    fn fixture() -> (TempDir, Arc<ImRepository>, ImDelivery) {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let channel = ImChannelKind::WeCom;
        let kind = ImNotificationKind::RunFailure;
        let destination = ImDestination {
            destination_id: "user-1".into(),
            conversation_id: "chat-1".into(),
            authorized_actor_id: "actor-1".into(),
        };
        let delivery = ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, "user-1", kind, "event-1"),
            channel,
            destination,
            notification_kind: kind,
            canonical_event_id: "event-1".into(),
            payload: ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: "event-1".into(),
                    notification_kind: kind,
                    locator: ImNavigationLocator {
                        project_id: "project-1".into(),
                        task_id: Some("task-1".into()),
                        run_id: Some("run-1".into()),
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
            expires_at_ms: i64::MAX,
            display_ref: None,
        };
        repository.enqueue(&delivery, 100).unwrap();
        (temp, repository, delivery)
    }

    #[tokio::test]
    async fn worker_releases_sqlite_before_network_and_settles_delivery() {
        let (_temp, repository, delivery) = fixture();
        let connector = Arc::new(FakeConnector {
            sends: AtomicUsize::new(0),
        });
        let worker = ImDeliveryWorker::new(
            ImChannelKind::WeCom,
            Arc::clone(&repository),
            connector.clone(),
        )
        .with_limits(1, Duration::from_secs(30));
        assert_eq!(worker.dispatch_once(100).await.unwrap(), 1);
        assert_eq!(connector.sends.load(Ordering::SeqCst), 1);
        assert!(
            repository
                .claim_due(ImChannelKind::WeCom, 101, Duration::from_secs(30), 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(delivery.canonical_event_id, "event-1");
    }

    #[tokio::test]
    async fn worker_cancellation_interrupts_a_blocked_connector_send() {
        let (_temp, repository, _delivery) = fixture();
        let connector = Arc::new(BlockingConnector {
            started: tokio::sync::Notify::new(),
        });
        let worker = ImDeliveryWorker::new(
            ImChannelKind::WeCom,
            Arc::clone(&repository),
            connector.clone(),
        );
        let (sender, receiver) = im_worker_channel();
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let started = connector.started.notified();
        let task = tokio::spawn(worker.run(receiver, task_cancellation));

        sender
            .send(ImWorkerSignal::DeliveryAvailable)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), started)
            .await
            .expect("connector send started");
        cancellation.cancel();

        tokio::time::timeout(Duration::from_millis(200), task)
            .await
            .expect("worker must converge after cancellation")
            .unwrap();
    }
}
