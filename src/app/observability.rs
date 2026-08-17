use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, RwLock};

use crate::app::RuntimeLifecycleEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriberMode {
    Async,
    Inline,
}

#[derive(Clone)]
struct RuntimeLifecycleSubscriber {
    mode: SubscriberMode,
    handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
}

#[derive(Default)]
struct RuntimeLifecycleSubscribers {
    entries: Vec<RuntimeLifecycleSubscriber>,
    names: HashSet<&'static str>,
}

#[derive(Clone, Default)]
pub struct RuntimeLifecycleBus {
    subscribers: Arc<RwLock<RuntimeLifecycleSubscribers>>,
}

impl RuntimeLifecycleBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>) {
        self.subscribe_with_mode(SubscriberMode::Async, handler);
    }

    pub fn subscribe_inline(&self, handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>) {
        self.subscribe_with_mode(SubscriberMode::Inline, handler);
    }

    pub fn subscribe_named(
        &self,
        name: &'static str,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> bool {
        self.subscribe_named_with_mode(name, SubscriberMode::Async, handler)
    }

    pub fn subscribe_named_with_mode(
        &self,
        name: &'static str,
        mode: SubscriberMode,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) -> bool {
        let mut subscribers = self
            .subscribers
            .write()
            .expect("RuntimeLifecycleBus subscriber state poisoned; this is a bug");
        if !subscribers.names.insert(name) {
            return false;
        }
        subscribers
            .entries
            .push(RuntimeLifecycleSubscriber { mode, handler });
        true
    }

    pub fn subscribe_with_mode(
        &self,
        mode: SubscriberMode,
        handler: Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>,
    ) {
        self.subscribers
            .write()
            .expect("RuntimeLifecycleBus subscriber state poisoned; this is a bug")
            .entries
            .push(RuntimeLifecycleSubscriber { mode, handler });
    }

    pub fn emit(&self, event: RuntimeLifecycleEvent) {
        // Observability must not unwind a canonical task transition. Recover
        // the last subscriber snapshot even if an earlier registration panic
        // poisoned the lock.
        let subscribers = match self.subscribers.read() {
            Ok(subscribers) => subscribers.entries.clone(),
            Err(poisoned) => poisoned.into_inner().entries.clone(),
        };
        for subscriber in subscribers {
            let event = event.clone();
            match subscriber.mode {
                SubscriberMode::Async => {
                    let handler = subscriber.handler;
                    let run = move || {
                        let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                            handler(event);
                        }));
                    };
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move { run() });
                    } else {
                        let _ = std::thread::Builder::new()
                            .name("runtime-lifecycle-subscriber".to_string())
                            .spawn(run);
                    }
                }
                SubscriberMode::Inline => {
                    let handler = subscriber.handler;
                    let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                        handler(event);
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PauseReason;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample_event() -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunPaused {
            event_id: "event-1".to_string(),
            occurred_at: "2026-01-01T00:00:00".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-1".to_string(),
            task_id: "task-1".to_string(),
            run_id: "run-1".to_string(),
            round_id: "round-1".to_string(),
            node_id: "node-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            node_label: "node".to_string(),
            pause_reason: PauseReason::ProcessInterrupted,
            task_title: None,
        }
    }

    #[test]
    fn inline_subscriber_panic_does_not_stop_other_subscribers() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(|_| panic!("subscriber panic")));
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cloned_bus_shares_subscribers() {
        let bus = RuntimeLifecycleBus::new();
        let cloned = bus.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        cloned.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn named_subscriber_is_registered_once_across_clones() {
        let bus = RuntimeLifecycleBus::new();
        let cloned = bus.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let first_count = count.clone();
        let duplicate_count = count.clone();

        assert!(bus.subscribe_named_with_mode(
            "desktop.metrics",
            SubscriberMode::Inline,
            Arc::new(move |_| {
                first_count.fetch_add(1, Ordering::SeqCst);
            }),
        ));
        assert!(!cloned.subscribe_named_with_mode(
            "desktop.metrics",
            SubscriberMode::Inline,
            Arc::new(move |_| {
                duplicate_count.fetch_add(100, Ordering::SeqCst);
            }),
        ));

        cloned.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn different_named_subscribers_are_all_registered() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        for name in ["desktop.metrics", "desktop.notifications"] {
            let count = count.clone();
            assert!(bus.subscribe_named_with_mode(
                name,
                SubscriberMode::Inline,
                Arc::new(move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                }),
            ));
        }

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn poisoned_subscriber_lock_does_not_unwind_publisher() {
        let bus = RuntimeLifecycleBus::new();
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_handler = count.clone();
        bus.subscribe_inline(Arc::new(move |_| {
            count_for_handler.fetch_add(1, Ordering::SeqCst);
        }));

        let subscribers = bus.subscribers.clone();
        let _ = panic::catch_unwind(AssertUnwindSafe(move || {
            let _guard = subscribers.write().unwrap();
            panic!("poison subscriber registry");
        }));

        bus.emit(sample_event());

        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
