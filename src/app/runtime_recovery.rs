use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

use crate::storage::GoldBandPaths;
use crate::storage::core_state::{CoreStateDatabase, CoreStateError, RuntimeRecoveryCandidate};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeRunKey {
    project_id: String,
    task_id: String,
    run_id: String,
}

impl RuntimeRunKey {
    fn new(paths: &GoldBandPaths, task_id: &str, run_id: &str) -> Self {
        Self {
            project_id: paths.project_id.clone(),
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
        }
    }

    fn from_candidate(candidate: &RuntimeRecoveryCandidate) -> Self {
        Self {
            project_id: candidate.project_id.clone(),
            task_id: candidate.task_id.clone(),
            run_id: candidate.run_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeRecoveryPhase {
    Recovering,
    Accepting,
    Failed,
    ShuttingDown,
}

#[derive(Debug)]
struct ActiveRuntimeRegistry {
    phase: RuntimeRecoveryPhase,
    blocked_project_ids: HashSet<String>,
    active: HashMap<RuntimeRunKey, RuntimeRecoveryCandidate>,
}

impl Default for ActiveRuntimeRegistry {
    fn default() -> Self {
        Self {
            phase: RuntimeRecoveryPhase::Recovering,
            blocked_project_ids: HashSet::new(),
            active: HashMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeRecoveryError {
    #[error(transparent)]
    CoreState(#[from] CoreStateError),
    #[error("runtime recovery has not completed")]
    RecoveryInProgress,
    #[error("runtime startup recovery failed")]
    StartupRecoveryFailed,
    #[error("runtime recovery failed for project {project_id}")]
    ProjectBlocked { project_id: String },
    #[error("application shutdown has started")]
    ShuttingDown,
    #[error("runtime recovery registry lock is unavailable")]
    RegistryUnavailable,
}

impl RuntimeRecoveryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CoreState(error) => error.code(),
            Self::RecoveryInProgress => "runtime.recovery-in-progress",
            Self::StartupRecoveryFailed => "runtime.startup-recovery-failed",
            Self::ProjectBlocked { .. } => "runtime.workspace-recovery-blocked",
            Self::ShuttingDown => "runtime.app-shutting-down",
            Self::RegistryUnavailable => "runtime.recovery-registry-unavailable",
        }
    }

    pub fn params(&self) -> Value {
        match self {
            Self::CoreState(error) => error.params(),
            Self::ProjectBlocked { project_id } => {
                serde_json::json!({ "projectId": project_id })
            }
            Self::RecoveryInProgress
            | Self::StartupRecoveryFailed
            | Self::ShuttingDown
            | Self::RegistryUnavailable => {
                serde_json::json!({})
            }
        }
    }
}

#[derive(Debug)]
pub struct RuntimeRecoveryCoordinator {
    database: CoreStateDatabase,
    runtime_instance_id: String,
    registry: Mutex<ActiveRuntimeRegistry>,
    /// 相位离开 Recovering 时唤醒 `wait_for_startup_accepting` 等待者。
    phase_change: Condvar,
}

impl RuntimeRecoveryCoordinator {
    pub fn new(core_db_path: camino::Utf8PathBuf) -> Arc<Self> {
        Arc::new(Self {
            database: CoreStateDatabase::new(core_db_path),
            runtime_instance_id: format!("desktop-{}", uuid::Uuid::new_v4().simple()),
            registry: Mutex::new(ActiveRuntimeRegistry::default()),
            phase_change: Condvar::new(),
        })
    }

    #[cfg(test)]
    fn with_candidate_limit(
        core_db_path: camino::Utf8PathBuf,
        candidate_limit: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            database: CoreStateDatabase::with_candidate_limit(core_db_path, candidate_limit),
            runtime_instance_id: format!("desktop-{}", uuid::Uuid::new_v4().simple()),
            registry: Mutex::new(ActiveRuntimeRegistry::default()),
            phase_change: Condvar::new(),
        })
    }

    pub fn list_persisted_candidates(
        &self,
    ) -> Result<Vec<RuntimeRecoveryCandidate>, RuntimeRecoveryError> {
        Ok(self.database.list_runtime_recovery_candidates()?)
    }

    pub fn ensure_scheduler_start_allowed(&self) -> Result<(), RuntimeRecoveryError> {
        let registry = self
            .registry
            .lock()
            .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
        match registry.phase {
            RuntimeRecoveryPhase::Recovering => Err(RuntimeRecoveryError::RecoveryInProgress),
            RuntimeRecoveryPhase::Accepting => Ok(()),
            RuntimeRecoveryPhase::Failed => Err(RuntimeRecoveryError::StartupRecoveryFailed),
            RuntimeRecoveryPhase::ShuttingDown => Err(RuntimeRecoveryError::ShuttingDown),
        }
    }

    /// 阻塞等待启动恢复完成（相位离开 Recovering）：Accepting → Ok；Failed / ShuttingDown → Err；
    /// 超时仍 Recovering → Err(RecoveryInProgress)。供 Multica 断点续跑在判定 resume 语义前等待
    /// 启动管线完成定点恢复（恢复未完成时 checkpoint 指向的 run 状态尚未收敛）。
    pub fn wait_for_startup_accepting(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), RuntimeRecoveryError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
        let deadline = std::time::Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(std::time::Instant::now);
        loop {
            match registry.phase {
                RuntimeRecoveryPhase::Accepting => return Ok(()),
                RuntimeRecoveryPhase::Failed => {
                    return Err(RuntimeRecoveryError::StartupRecoveryFailed);
                }
                RuntimeRecoveryPhase::ShuttingDown => {
                    return Err(RuntimeRecoveryError::ShuttingDown);
                }
                RuntimeRecoveryPhase::Recovering => {}
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(RuntimeRecoveryError::RecoveryInProgress);
            }
            let (guard, _) = self
                .phase_change
                .wait_timeout(registry, deadline - now)
                .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
            registry = guard;
        }
    }

    pub fn begin(
        self: &Arc<Self>,
        paths: &GoldBandPaths,
        task_id: &str,
        run_id: &str,
    ) -> Result<RuntimeCandidateRegistration, RuntimeRecoveryError> {
        {
            let registry = self
                .registry
                .lock()
                .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
            ensure_project_accepting(&registry, &paths.project_id)?;
        }

        let candidate = RuntimeRecoveryCandidate::new(
            paths.repo_root.to_string(),
            paths.project_id.clone(),
            task_id,
            run_id,
            uuid::Uuid::new_v4().to_string(),
            self.runtime_instance_id.clone(),
        );
        self.database
            .upsert_runtime_recovery_candidate(&candidate)?;

        let key = RuntimeRunKey::from_candidate(&candidate);
        let accepted = {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
            match ensure_project_accepting(&registry, &candidate.project_id) {
                Ok(()) => {
                    registry.active.insert(key, candidate.clone());
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        if let Err(error) = accepted {
            let _ = self.database.delete_runtime_recovery_candidate(
                &candidate.project_id,
                &candidate.task_id,
                &candidate.run_id,
                &candidate.candidate_token,
            );
            return Err(error);
        }

        Ok(RuntimeCandidateRegistration {
            coordinator: self.clone(),
            candidate,
            committed: false,
        })
    }

    pub fn finish(
        &self,
        paths: &GoldBandPaths,
        task_id: &str,
        run_id: &str,
        candidate_token: Option<&str>,
    ) -> Result<bool, RuntimeRecoveryError> {
        let Some(candidate_token) = candidate_token else {
            return Ok(false);
        };
        let key = RuntimeRunKey::new(paths, task_id, run_id);
        {
            let mut registry = self
                .registry
                .lock()
                .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
            if registry
                .active
                .get(&key)
                .is_some_and(|active| active.candidate_token == candidate_token)
            {
                registry.active.remove(&key);
            }
        }
        Ok(self.database.delete_runtime_recovery_candidate(
            &paths.project_id,
            task_id,
            run_id,
            candidate_token,
        )?)
    }

    pub fn consume_persisted_candidate(
        &self,
        candidate: &RuntimeRecoveryCandidate,
    ) -> Result<bool, RuntimeRecoveryError> {
        let key = RuntimeRunKey::from_candidate(candidate);
        if let Ok(mut registry) = self.registry.lock()
            && registry
                .active
                .get(&key)
                .is_some_and(|active| active.candidate_token == candidate.candidate_token)
        {
            registry.active.remove(&key);
        }
        Ok(self.database.delete_runtime_recovery_candidate(
            &candidate.project_id,
            &candidate.task_id,
            &candidate.run_id,
            &candidate.candidate_token,
        )?)
    }

    pub fn complete_startup_recovery(
        &self,
        blocked_project_ids: HashSet<String>,
    ) -> Result<(), RuntimeRecoveryError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
        match registry.phase {
            RuntimeRecoveryPhase::Failed => {
                return Err(RuntimeRecoveryError::StartupRecoveryFailed);
            }
            RuntimeRecoveryPhase::ShuttingDown => {
                return Err(RuntimeRecoveryError::ShuttingDown);
            }
            RuntimeRecoveryPhase::Recovering | RuntimeRecoveryPhase::Accepting => {}
        }
        registry.blocked_project_ids = blocked_project_ids;
        registry.phase = RuntimeRecoveryPhase::Accepting;
        self.phase_change.notify_all();
        Ok(())
    }

    /// 以失败终态结束启动恢复并唤醒所有等待者。
    ///
    /// 转换是幂等且单调的：仅 Recovering 可进入 Failed；已经 Accepting 时忽略迟到失败，
    /// Failed 重复调用保持原状态，ShuttingDown 不允许回退。
    pub fn fail_startup_recovery(&self) -> Result<(), RuntimeRecoveryError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
        match registry.phase {
            RuntimeRecoveryPhase::Recovering => {
                registry.phase = RuntimeRecoveryPhase::Failed;
                self.phase_change.notify_all();
            }
            RuntimeRecoveryPhase::Accepting | RuntimeRecoveryPhase::Failed => {}
            RuntimeRecoveryPhase::ShuttingDown => {
                return Err(RuntimeRecoveryError::ShuttingDown);
            }
        }
        Ok(())
    }

    pub fn begin_shutdown(&self) -> Result<Vec<RuntimeRecoveryCandidate>, RuntimeRecoveryError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| RuntimeRecoveryError::RegistryUnavailable)?;
        registry.phase = RuntimeRecoveryPhase::ShuttingDown;
        self.phase_change.notify_all();
        Ok(registry.active.values().cloned().collect())
    }

    fn abandon_uncommitted(&self, candidate: &RuntimeRecoveryCandidate) {
        let key = RuntimeRunKey::from_candidate(candidate);
        if let Ok(mut registry) = self.registry.lock()
            && registry
                .active
                .get(&key)
                .is_some_and(|active| active.candidate_token == candidate.candidate_token)
        {
            registry.active.remove(&key);
        }
    }
}

fn ensure_project_accepting(
    registry: &ActiveRuntimeRegistry,
    project_id: &str,
) -> Result<(), RuntimeRecoveryError> {
    match registry.phase {
        RuntimeRecoveryPhase::Recovering => Err(RuntimeRecoveryError::RecoveryInProgress),
        RuntimeRecoveryPhase::Failed => Err(RuntimeRecoveryError::StartupRecoveryFailed),
        RuntimeRecoveryPhase::ShuttingDown => Err(RuntimeRecoveryError::ShuttingDown),
        RuntimeRecoveryPhase::Accepting => {
            if registry.blocked_project_ids.contains(project_id) {
                Err(RuntimeRecoveryError::ProjectBlocked {
                    project_id: project_id.to_string(),
                })
            } else {
                Ok(())
            }
        }
    }
}

pub struct RuntimeCandidateRegistration {
    coordinator: Arc<RuntimeRecoveryCoordinator>,
    candidate: RuntimeRecoveryCandidate,
    committed: bool,
}

impl std::fmt::Debug for RuntimeCandidateRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeCandidateRegistration")
            .field("candidate", &self.candidate)
            .field("committed", &self.committed)
            .finish_non_exhaustive()
    }
}

impl RuntimeCandidateRegistration {
    pub fn token(&self) -> &str {
        &self.candidate.candidate_token
    }

    pub fn commit(mut self) {
        self.committed = true;
    }

    pub fn abort(mut self) -> Result<(), RuntimeRecoveryError> {
        self.coordinator.abandon_uncommitted(&self.candidate);
        self.coordinator
            .database
            .delete_runtime_recovery_candidate(
                &self.candidate.project_id,
                &self.candidate.task_id,
                &self.candidate.run_id,
                &self.candidate.candidate_token,
            )?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for RuntimeCandidateRegistration {
    fn drop(&mut self) {
        if !self.committed {
            self.coordinator.abandon_uncommitted(&self.candidate);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{GoldBandPaths, StoragePathConfig};

    fn paths(directory: &tempfile::TempDir, workspace: &str) -> GoldBandPaths {
        let root = camino::Utf8PathBuf::from_path_buf(directory.path().join(workspace)).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        GoldBandPaths::new_with_path_config(
            root,
            StoragePathConfig {
                app_key: "runtime-recovery-test",
                config_dir_name: ".gold-band",
                home_env_var: "GOLD_BAND_RUNTIME_RECOVERY_TEST_HOME",
            },
        )
    }

    #[test]
    fn startup_gate_blocks_registration_until_recovery_completes() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());

        let error = coordinator
            .begin(&paths, "task-001", "run-001")
            .unwrap_err();
        assert!(matches!(error, RuntimeRecoveryError::RecoveryInProgress));
        assert!(matches!(
            coordinator.ensure_scheduler_start_allowed(),
            Err(RuntimeRecoveryError::RecoveryInProgress)
        ));

        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();
        coordinator.ensure_scheduler_start_allowed().unwrap();
        coordinator
            .begin(&paths, "task-001", "run-001")
            .unwrap()
            .commit();
        assert_eq!(coordinator.begin_shutdown().unwrap().len(), 1);
        assert!(matches!(
            coordinator.ensure_scheduler_start_allowed(),
            Err(RuntimeRecoveryError::ShuttingDown)
        ));
    }

    #[test]
    fn dropping_uncommitted_registration_only_removes_the_active_projection() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();

        drop(coordinator.begin(&paths, "task-001", "run-001").unwrap());

        assert!(coordinator.begin_shutdown().unwrap().is_empty());
        assert_eq!(coordinator.list_persisted_candidates().unwrap().len(), 1);
    }

    #[test]
    fn shutdown_snapshot_contains_only_process_active_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();

        drop(
            coordinator
                .begin(&paths, "task-stale", "run-stale")
                .unwrap(),
        );
        coordinator
            .begin(&paths, "task-active", "run-active")
            .unwrap()
            .commit();

        let active = coordinator.begin_shutdown().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].task_id, "task-active");
        assert_eq!(coordinator.list_persisted_candidates().unwrap().len(), 2);
    }

    #[test]
    fn canonical_pause_cleanup_leaves_no_candidate_for_the_next_startup() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();
        let registration = coordinator.begin(&paths, "task-001", "run-001").unwrap();
        let token = registration.token().to_string();
        registration.commit();

        assert_eq!(coordinator.begin_shutdown().unwrap().len(), 1);
        assert!(
            coordinator
                .finish(&paths, "task-001", "run-001", Some(&token))
                .unwrap()
        );
        assert!(coordinator.begin_shutdown().unwrap().is_empty());
        assert!(coordinator.list_persisted_candidates().unwrap().is_empty());

        let reopened = RuntimeRecoveryCoordinator::new(paths.core_db_path());
        assert!(reopened.list_persisted_candidates().unwrap().is_empty());
    }

    #[test]
    fn stale_finish_cannot_remove_a_new_generation() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();

        let old = coordinator.begin(&paths, "task-001", "run-001").unwrap();
        let old_token = old.token().to_string();
        old.commit();
        let new = coordinator.begin(&paths, "task-001", "run-001").unwrap();
        let new_token = new.token().to_string();
        new.commit();

        assert!(
            !coordinator
                .finish(&paths, "task-001", "run-001", Some(&old_token))
                .unwrap()
        );
        let persisted = coordinator.list_persisted_candidates().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].candidate_token, new_token);
        let active = coordinator.begin_shutdown().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].candidate_token, new_token);
    }

    #[test]
    fn wait_for_startup_accepting_blocks_until_phase_change_or_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());

        // Recovering 相位：超时返回 RecoveryInProgress（不永久阻塞调用方）。
        assert!(matches!(
            coordinator.wait_for_startup_accepting(std::time::Duration::from_millis(20)),
            Err(RuntimeRecoveryError::RecoveryInProgress)
        ));

        // 相位翻转后立即放行；shutdown 同样唤醒等待者并返回 ShuttingDown。
        let waiter = coordinator.clone();
        let handle = std::thread::spawn(move || {
            waiter
                .wait_for_startup_accepting(std::time::Duration::from_secs(5))
                .unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();
        handle.join().unwrap();
        coordinator.begin_shutdown().unwrap();
        assert!(matches!(
            coordinator.wait_for_startup_accepting(std::time::Duration::from_secs(5)),
            Err(RuntimeRecoveryError::ShuttingDown)
        ));
    }

    #[test]
    fn startup_failure_is_terminal_and_wakes_waiters() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::new(paths.core_db_path());

        let waiter = coordinator.clone();
        let handle = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let result = waiter.wait_for_startup_accepting(std::time::Duration::from_secs(5));
            (started.elapsed(), result)
        });
        std::thread::sleep(std::time::Duration::from_millis(50));

        coordinator.fail_startup_recovery().unwrap();

        let (elapsed, result) = handle.join().unwrap();
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "startup failure should wake waiters immediately, elapsed={elapsed:?}"
        );
        let error = result.unwrap_err();
        assert!(matches!(error, RuntimeRecoveryError::StartupRecoveryFailed));
        assert_eq!(error.code(), "runtime.startup-recovery-failed");
        coordinator.fail_startup_recovery().unwrap();
        assert!(matches!(
            coordinator.complete_startup_recovery(HashSet::new()),
            Err(RuntimeRecoveryError::StartupRecoveryFailed)
        ));
        assert!(matches!(
            coordinator.ensure_scheduler_start_allowed(),
            Err(RuntimeRecoveryError::StartupRecoveryFailed)
        ));
        assert!(matches!(
            coordinator.begin(&paths, "task-001", "run-001"),
            Err(RuntimeRecoveryError::StartupRecoveryFailed)
        ));
    }

    #[test]
    fn failed_candidate_insert_never_enters_the_active_registry() {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory, "workspace");
        let coordinator = RuntimeRecoveryCoordinator::with_candidate_limit(paths.core_db_path(), 0);
        coordinator
            .complete_startup_recovery(HashSet::new())
            .unwrap();

        let error = coordinator
            .begin(&paths, "task-001", "run-001")
            .unwrap_err();

        assert!(matches!(
            error,
            RuntimeRecoveryError::CoreState(CoreStateError::RuntimeRecoveryCapacity { limit: 0 })
        ));
        assert!(coordinator.begin_shutdown().unwrap().is_empty());
        assert!(coordinator.list_persisted_candidates().unwrap().is_empty());
    }
}
