use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::error::{self, ExecutionPlanError, PlanResult};

struct ShardInner {
    readers: usize,
    writer: bool,
}

struct Shard {
    inner: Mutex<ShardInner>,
    cv: Condvar,
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Shard>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<Shard>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn lock_key(project_id: &str, task_id: &str, run_id: &str) -> String {
    format!("{project_id}/{task_id}/{run_id}")
}

pub fn authoring_lock_key(project_id: &str, task_id: &str) -> String {
    format!("authoring/{project_id}/{task_id}")
}

fn shard(key: &str) -> PlanResult<Arc<Shard>> {
    let mut registry = registry()
        .lock()
        .map_err(|_| poisoned("execution-plan-lock"))?;
    Ok(registry
        .entry(key.to_string())
        .or_insert_with(|| {
            Arc::new(Shard {
                inner: Mutex::new(ShardInner {
                    readers: 0,
                    writer: false,
                }),
                cv: Condvar::new(),
            })
        })
        .clone())
}

fn poisoned(scope: &str) -> ExecutionPlanError {
    ExecutionPlanError::new(
        error::RECOVERY_REQUIRED,
        serde_json::json!({ "scope": scope }),
    )
}

pub struct DispatchReadGuard {
    shard: Arc<Shard>,
}

impl Drop for DispatchReadGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.shard.inner.lock() {
            inner.readers = inner.readers.saturating_sub(1);
            self.shard.cv.notify_all();
        }
    }
}

pub struct PlanWriteGuard {
    shard: Arc<Shard>,
}

impl Drop for PlanWriteGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.shard.inner.lock() {
            inner.writer = false;
            self.shard.cv.notify_all();
        }
    }
}

/// Dispatch readers wait only while a publish is inside its short critical section.
/// A publish fails immediately when a dispatch reader is already active.
pub fn acquire_dispatch_read(key: &str) -> PlanResult<DispatchReadGuard> {
    let shard = shard(key)?;
    let mut inner = shard
        .inner
        .lock()
        .map_err(|_| poisoned("execution-plan-lock"))?;
    while inner.writer {
        inner = shard
            .cv
            .wait(inner)
            .map_err(|_| poisoned("execution-plan-lock"))?;
    }
    inner.readers = inner.readers.saturating_add(1);
    drop(inner);
    Ok(DispatchReadGuard { shard })
}

pub fn try_acquire_plan_write(key: &str) -> PlanResult<PlanWriteGuard> {
    let shard = shard(key)?;
    let mut inner = shard
        .inner
        .lock()
        .map_err(|_| poisoned("execution-plan-lock"))?;
    if inner.readers > 0 || inner.writer {
        return Err(ExecutionPlanError::new(
            error::REVISION_CONFLICT,
            serde_json::json!({ "reason": "dispatch-in-progress" }),
        ));
    }
    inner.writer = true;
    drop(inner);
    Ok(PlanWriteGuard { shard })
}

pub fn acquire_plan_write_wait(key: &str) -> PlanResult<PlanWriteGuard> {
    let shard = shard(key)?;
    let mut inner = shard
        .inner
        .lock()
        .map_err(|_| poisoned("execution-plan-lock"))?;
    while inner.readers > 0 || inner.writer {
        inner = shard
            .cv
            .wait(inner)
            .map_err(|_| poisoned("execution-plan-lock"))?;
    }
    inner.writer = true;
    drop(inner);
    Ok(PlanWriteGuard { shard })
}

#[cfg(test)]
pub fn dispatch_read_held(key: &str) -> bool {
    let Ok(shard) = shard(key) else {
        return false;
    };
    shard
        .inner
        .lock()
        .map(|inner| inner.readers > 0)
        .unwrap_or(false)
}

/// Lock order: take the execution-plan lock before `attempt_runtime_state_lock`.
/// Provider callbacks keep using only the attempt runtime lock.
pub fn with_dispatch_read<T>(key: &str, body: impl FnOnce() -> T) -> PlanResult<T> {
    let guard = acquire_dispatch_read(key)?;
    let value = body();
    drop(guard);
    Ok(value)
}
