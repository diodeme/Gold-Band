use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, RwLock};

use crate::app::WorkflowEvent;

/// Lightweight in-process event bus for workflow lifecycle events.
///
/// Subscribers register via [`subscribe`] (typically during app setup, before
/// any workflow starts). The orchestrator publishes via [`emit`]. Each
/// subscriber is invoked inside `catch_unwind` — a panic in one subscriber
/// never affects other subscribers or the orchestrator.
///
/// # Constraints
///
/// - `subscribe()` acquires a write lock. Only call it during setup, not from
///   hot paths or inside a subscriber handler.
/// - Subscriber handlers **must not** call `subscribe()` or `emit()` on the
///   same `ObservabilityBus` instance — the read lock held by `emit()` would
///   deadlock with the write lock needed by `subscribe()`.
///
/// # Thread safety
///
/// `ObservabilityBus` is `Send + Sync`. `emit()` takes a read lock — multiple
/// threads can emit concurrently without blocking each other.
#[derive(Clone, Default)]
pub struct ObservabilityBus {
    subscribers: Arc<RwLock<Vec<Arc<dyn Fn(WorkflowEvent) + Send + Sync>>>>,
}

impl ObservabilityBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a subscriber. Takes a write lock — call during app init,
    /// not during workflow execution.
    pub fn subscribe(&self, handler: Arc<dyn Fn(WorkflowEvent) + Send + Sync>) {
        self.subscribers
            .write()
            .expect("ObservabilityBus subscriber list poisoned; this is a bug")
            .push(handler);
    }

    /// Publish an event to all subscribers. Each subscriber is wrapped in
    /// `catch_unwind`. Takes a read lock for the duration of the call.
    ///
    /// The event is cloned once per subscriber. With the current single
    /// subscriber (metrics), this is one clone per emit — negligible. If
    /// subscriber count grows beyond ~10, consider switching the handler
    /// signature to `Fn(Arc<WorkflowEvent>)` to avoid per-subscriber clones.
    pub fn emit(&self, event: WorkflowEvent) {
        let subs = self
            .subscribers
            .read()
            .expect("ObservabilityBus subscriber list poisoned; this is a bug");
        for sub in subs.iter() {
            let sub = Arc::clone(sub);
            let event = event.clone();
            let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                sub(event);
            }));
        }
    }
}
