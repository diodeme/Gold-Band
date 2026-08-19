use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::GoldBandPaths;

const CORE_SCHEMA_VERSION: i64 = 2;
pub const WORKSPACE_IDENTITY_SCHEMA_VERSION: i64 = 2;
pub const RUNTIME_RECOVERY_CANDIDATE_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRecoveryCandidate {
    pub workspace_path: String,
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub candidate_token: String,
    pub runtime_instance_id: String,
    pub registered_at_ms: i64,
}

impl RuntimeRecoveryCandidate {
    pub fn new(
        workspace_path: impl Into<String>,
        project_id: impl Into<String>,
        task_id: impl Into<String>,
        run_id: impl Into<String>,
        candidate_token: impl Into<String>,
        runtime_instance_id: impl Into<String>,
    ) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            project_id: project_id.into(),
            task_id: task_id.into(),
            run_id: run_id.into(),
            candidate_token: candidate_token.into(),
            runtime_instance_id: runtime_instance_id.into(),
            registered_at_ms: now_millis(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreStateError {
    #[error("core state I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("core state SQLite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("core state lock is unavailable")]
    LockUnavailable,
    #[error("core state schema version {found} is newer than supported version {supported}")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("runtime recovery candidate capacity {limit} was reached")]
    RuntimeRecoveryCapacity { limit: usize },
}

impl CoreStateError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RuntimeRecoveryCapacity { .. } => "runtime.recovery-candidate-capacity",
            Self::SchemaTooNew { .. } => "runtime.core-state-schema-too-new",
            Self::Io(_) | Self::Sqlite(_) | Self::LockUnavailable => {
                "runtime.core-state-unavailable"
            }
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::RuntimeRecoveryCapacity { limit } => serde_json::json!({ "limit": limit }),
            Self::SchemaTooNew { found, supported } => {
                serde_json::json!({ "found": found, "supported": supported })
            }
            Self::Io(_) | Self::Sqlite(_) | Self::LockUnavailable => serde_json::json!({}),
        }
    }
}

pub struct CoreStateDatabase {
    path: Utf8PathBuf,
    connection: Mutex<Option<Connection>>,
    candidate_limit: usize,
}

impl std::fmt::Debug for CoreStateDatabase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CoreStateDatabase")
            .field("path", &self.path)
            .field("candidate_limit", &self.candidate_limit)
            .finish_non_exhaustive()
    }
}

impl CoreStateDatabase {
    pub fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            connection: Mutex::new(None),
            candidate_limit: RUNTIME_RECOVERY_CANDIDATE_LIMIT,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_candidate_limit(path: Utf8PathBuf, candidate_limit: usize) -> Self {
        Self {
            path,
            connection: Mutex::new(None),
            candidate_limit,
        }
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub fn upsert_runtime_recovery_candidate(
        &self,
        candidate: &RuntimeRecoveryCandidate,
    ) -> Result<(), CoreStateError> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let exists = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM runtime_recovery_candidates
                    WHERE project_id = ?1 AND task_id = ?2 AND run_id = ?3
                )",
                params![candidate.project_id, candidate.task_id, candidate.run_id],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                let count = transaction.query_row(
                    "SELECT COUNT(*) FROM runtime_recovery_candidates",
                    [],
                    |row| row.get::<_, i64>(0),
                )?;
                if count >= self.candidate_limit as i64 {
                    return Err(CoreStateError::RuntimeRecoveryCapacity {
                        limit: self.candidate_limit,
                    });
                }
            }
            transaction.execute(
                "INSERT INTO runtime_recovery_candidates (
                    project_id, workspace_path, task_id, run_id,
                    candidate_token, runtime_instance_id, registered_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(project_id, task_id, run_id) DO UPDATE SET
                    workspace_path = excluded.workspace_path,
                    candidate_token = excluded.candidate_token,
                    runtime_instance_id = excluded.runtime_instance_id,
                    registered_at_ms = excluded.registered_at_ms",
                params![
                    candidate.project_id,
                    candidate.workspace_path,
                    candidate.task_id,
                    candidate.run_id,
                    candidate.candidate_token,
                    candidate.runtime_instance_id,
                    candidate.registered_at_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn delete_runtime_recovery_candidate(
        &self,
        project_id: &str,
        task_id: &str,
        run_id: &str,
        candidate_token: &str,
    ) -> Result<bool, CoreStateError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "DELETE FROM runtime_recovery_candidates
                 WHERE project_id = ?1 AND task_id = ?2 AND run_id = ?3
                   AND candidate_token = ?4",
                params![project_id, task_id, run_id, candidate_token],
            )?;
            Ok(changed > 0)
        })
    }

    pub fn list_runtime_recovery_candidates(
        &self,
    ) -> Result<Vec<RuntimeRecoveryCandidate>, CoreStateError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT project_id, workspace_path, task_id, run_id,
                        candidate_token, runtime_instance_id, registered_at_ms
                 FROM runtime_recovery_candidates
                 ORDER BY registered_at_ms, project_id, task_id, run_id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(RuntimeRecoveryCandidate {
                    project_id: row.get(0)?,
                    workspace_path: row.get(1)?,
                    task_id: row.get(2)?,
                    run_id: row.get(3)?,
                    candidate_token: row.get(4)?,
                    runtime_instance_id: row.get(5)?,
                    registered_at_ms: row.get(6)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
    }

    pub fn workspace_identity_version(&self) -> Result<Option<i64>, CoreStateError> {
        self.with_connection(|connection| {
            let version = connection
                .query_row(
                    "SELECT version FROM core_schema WHERE component = 'workspace_identity'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if version.is_some_and(|found| found > WORKSPACE_IDENTITY_SCHEMA_VERSION) {
                return Err(CoreStateError::SchemaTooNew {
                    found: version.unwrap_or_default(),
                    supported: WORKSPACE_IDENTITY_SCHEMA_VERSION,
                });
            }
            Ok(version)
        })
    }

    pub fn mark_workspace_identity_migrated(&self) -> Result<(), CoreStateError> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "INSERT INTO core_schema(component, version) VALUES ('workspace_identity', ?1)
                 ON CONFLICT(component) DO UPDATE SET version = excluded.version",
                params![WORKSPACE_IDENTITY_SCHEMA_VERSION],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, CoreStateError>,
    ) -> Result<T, CoreStateError> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| CoreStateError::LockUnavailable)?;
        if guard.is_none() {
            *guard = Some(open_connection(&self.path)?);
        }
        operation(guard.as_mut().expect("core state connection initialized"))
    }
}

fn open_connection(path: &Utf8Path) -> Result<Connection, CoreStateError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(path.as_std_path())?;
    connection.busy_timeout(std::time::Duration::from_secs(3))?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = FULL;",
    )?;
    ensure_schema(&mut connection)?;
    Ok(connection)
}

fn ensure_schema(connection: &mut Connection) -> Result<(), CoreStateError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS core_schema (
            component TEXT PRIMARY KEY NOT NULL,
            version INTEGER NOT NULL
         );",
    )?;
    let version = connection
        .query_row(
            "SELECT version FROM core_schema WHERE component = 'core'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if version.is_some_and(|found| found > CORE_SCHEMA_VERSION) {
        return Err(CoreStateError::SchemaTooNew {
            found: version.unwrap_or_default(),
            supported: CORE_SCHEMA_VERSION,
        });
    }
    if version == Some(CORE_SCHEMA_VERSION) {
        return Ok(());
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    match version.unwrap_or_default() {
        0 => create_runtime_recovery_candidates_v2(&transaction)?,
        1 => migrate_runtime_recovery_candidates_v1_to_v2(&transaction)?,
        _ => unreachable!("newer core schema was rejected above"),
    }
    transaction.execute(
        "INSERT INTO core_schema(component, version) VALUES ('core', ?1)
         ON CONFLICT(component) DO UPDATE SET version = excluded.version",
        params![CORE_SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(())
}

fn create_runtime_recovery_candidates_v2(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), rusqlite::Error> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_recovery_candidates (
            project_id TEXT NOT NULL,
            workspace_path TEXT NOT NULL,
            task_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            candidate_token TEXT NOT NULL,
            runtime_instance_id TEXT NOT NULL,
            registered_at_ms INTEGER NOT NULL,
            PRIMARY KEY (project_id, task_id, run_id)
         );",
    )
}

fn migrate_runtime_recovery_candidates_v1_to_v2(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), rusqlite::Error> {
    transaction.execute_batch(
        "ALTER TABLE runtime_recovery_candidates RENAME TO runtime_recovery_candidates_v1;",
    )?;
    create_runtime_recovery_candidates_v2(transaction)?;
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT workspace_path, task_id, run_id, candidate_token,
                    runtime_instance_id, registered_at_ms
             FROM runtime_recovery_candidates_v1
             ORDER BY registered_at_ms, workspace_key, task_id, run_id",
        )?;
        let mapped = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    for (workspace_path, task_id, run_id, token, instance_id, registered_at_ms) in rows {
        let project_id = GoldBandPaths::new(Utf8PathBuf::from(&workspace_path)).project_id;
        transaction.execute(
            "INSERT INTO runtime_recovery_candidates (
                project_id, workspace_path, task_id, run_id,
                candidate_token, runtime_instance_id, registered_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                project_id,
                workspace_path,
                task_id,
                run_id,
                token,
                instance_id,
                registered_at_ms,
            ],
        )?;
    }
    transaction.execute_batch("DROP TABLE runtime_recovery_candidates_v1;")?;
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(token: &str, task_id: &str, run_id: &str) -> RuntimeRecoveryCandidate {
        candidate_for_project("project-001", "C:/workspace", token, task_id, run_id)
    }

    fn candidate_for_project(
        project_id: &str,
        workspace_path: &str,
        token: &str,
        task_id: &str,
        run_id: &str,
    ) -> RuntimeRecoveryCandidate {
        RuntimeRecoveryCandidate::new(
            workspace_path,
            project_id,
            task_id,
            run_id,
            token,
            "instance-001",
        )
    }

    #[test]
    fn candidate_upsert_fences_stale_delete() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        let database = CoreStateDatabase::new(path);

        database
            .upsert_runtime_recovery_candidate(&candidate("old-token", "task-001", "run-001"))
            .unwrap();
        database
            .upsert_runtime_recovery_candidate(&candidate("new-token", "task-001", "run-001"))
            .unwrap();

        assert!(
            !database
                .delete_runtime_recovery_candidate(
                    "project-001",
                    "task-001",
                    "run-001",
                    "old-token"
                )
                .unwrap()
        );
        let rows = database.list_runtime_recovery_candidates().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].candidate_token, "new-token");
    }

    #[test]
    fn identical_run_locators_are_isolated_by_project_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        let database = CoreStateDatabase::new(path);
        database
            .upsert_runtime_recovery_candidate(&candidate_for_project(
                "project-001",
                "C:/workspace-a",
                "token-001",
                "task-001",
                "run-001",
            ))
            .unwrap();
        database
            .upsert_runtime_recovery_candidate(&candidate_for_project(
                "project-002",
                "C:/workspace-b",
                "token-002",
                "task-001",
                "run-001",
            ))
            .unwrap();

        assert!(
            database
                .delete_runtime_recovery_candidate(
                    "project-001",
                    "task-001",
                    "run-001",
                    "token-001",
                )
                .unwrap()
        );
        let remaining = database.list_runtime_recovery_candidates().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].project_id, "project-002");
        assert_eq!(remaining[0].candidate_token, "token-002");
    }

    #[test]
    fn consumed_candidate_is_absent_after_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        {
            let database = CoreStateDatabase::new(path.clone());
            database
                .upsert_runtime_recovery_candidate(&candidate(
                    "candidate-token",
                    "task-001",
                    "run-001",
                ))
                .unwrap();
            assert!(
                database
                    .delete_runtime_recovery_candidate(
                        "project-001",
                        "task-001",
                        "run-001",
                        "candidate-token"
                    )
                    .unwrap()
            );
        }

        let reopened = CoreStateDatabase::new(path);
        assert!(
            reopened
                .list_runtime_recovery_candidates()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn candidate_capacity_rejects_new_rows_without_evicting_active_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        let database = CoreStateDatabase::with_candidate_limit(path, 1);
        database
            .upsert_runtime_recovery_candidate(&candidate("token-001", "task-001", "run-001"))
            .unwrap();

        let error = database
            .upsert_runtime_recovery_candidate(&candidate("token-002", "task-002", "run-002"))
            .unwrap_err();
        assert!(matches!(
            error,
            CoreStateError::RuntimeRecoveryCapacity { limit: 1 }
        ));
        let rows = database.list_runtime_recovery_candidates().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_id, "task-001");
    }

    #[test]
    fn schema_v1_migrates_workspace_key_to_canonical_project_id() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("workspace")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        {
            let connection = Connection::open(path.as_std_path()).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE core_schema (
                        component TEXT PRIMARY KEY NOT NULL,
                        version INTEGER NOT NULL
                     );
                     INSERT INTO core_schema(component, version) VALUES ('core', 1);
                     CREATE TABLE runtime_recovery_candidates (
                        workspace_key TEXT NOT NULL,
                        workspace_path TEXT NOT NULL,
                        project_id TEXT NOT NULL,
                        task_id TEXT NOT NULL,
                        run_id TEXT NOT NULL,
                        candidate_token TEXT NOT NULL,
                        runtime_instance_id TEXT NOT NULL,
                        registered_at_ms INTEGER NOT NULL,
                        PRIMARY KEY (workspace_key, task_id, run_id)
                     );",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO runtime_recovery_candidates VALUES
                     (?1, ?2, 'legacy-project', 'task-001', 'run-001',
                      'token-001', 'instance-001', 1)",
                    params!["legacy-workspace-key", workspace.as_str()],
                )
                .unwrap();
        }

        let database = CoreStateDatabase::new(path.clone());
        let rows = database.list_runtime_recovery_candidates().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project_id, GoldBandPaths::new(workspace).project_id);
        drop(database);

        let connection = Connection::open(path.as_std_path()).unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(runtime_recovery_candidates)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!columns.iter().any(|column| column == "workspace_key"));
        assert_eq!(
            connection
                .query_row(
                    "SELECT version FROM core_schema WHERE component = 'core'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            CORE_SCHEMA_VERSION
        );
    }

    #[test]
    fn workspace_identity_marker_is_written_last_and_read_idempotently() {
        let directory = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(directory.path().join("core.db")).unwrap();
        let database = CoreStateDatabase::new(path);

        assert_eq!(database.workspace_identity_version().unwrap(), None);
        database.mark_workspace_identity_migrated().unwrap();
        database.mark_workspace_identity_migrated().unwrap();
        assert_eq!(
            database.workspace_identity_version().unwrap(),
            Some(WORKSPACE_IDENTITY_SCHEMA_VERSION)
        );
    }
}
