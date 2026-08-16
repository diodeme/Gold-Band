use super::ScheduledTaskDefinition;
use super::occurrence::{
    ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind, ScheduledError,
    ScheduledErrorCode, ScheduledOccurrence,
};
use super::store::{ScheduledTaskStore, ScheduledTriggerRecord};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 2;
const BUSY_TIMEOUT_MILLIS: u64 = 5_000;

pub const LEGACY_JSON_MIGRATION: &str = "legacy-json-v1";
pub const LEGACY_SHARED_DB_MIGRATION: &str = "legacy-shared-db-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJobRecord {
    pub definition: ScheduledTaskDefinition,
    pub revision: i64,
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableScheduledJob {
    pub job: ScheduledJobRecord,
    pub has_runnable_occurrence: bool,
    pub earliest_running_lease_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateJobResult {
    Updated(ScheduledJobRecord),
    Conflict(ScheduledJobRecord),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionResult {
    pub deleted: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJobDefinitionScan {
    pub definitions: Vec<ScheduledTaskDefinition>,
    pub invalid_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueMaterialization {
    Ready {
        job: ScheduledJobRecord,
        occurrence: ScheduledOccurrence,
    },
    NotDue,
    Stale,
    Disabled,
}

#[derive(Debug, Clone)]
pub struct LegacySchedulerSnapshot {
    pub definitions: Vec<ScheduledTaskDefinition>,
    pub triggers: BTreeMap<String, Vec<ScheduledTriggerRecord>>,
}

impl LegacySchedulerSnapshot {
    pub fn read_from(store: &ScheduledTaskStore) -> Result<Self> {
        let definitions = store
            .list()
            .map_err(|error| SchedulerDatabaseError::LegacyStore(error.to_string()))?;
        let mut triggers = BTreeMap::new();
        for definition in &definitions {
            triggers.insert(
                definition.id().to_string(),
                store
                    .list_triggers(definition.id())
                    .map_err(|error| SchedulerDatabaseError::LegacyStore(error.to_string()))?,
            );
        }
        Ok(Self {
            definitions,
            triggers,
        })
    }
}

#[derive(Debug, Error)]
pub enum SchedulerDatabaseError {
    #[error("scheduler sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("scheduler storage error: {0}")]
    Io(#[from] std::io::Error),
    #[error("scheduler JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("legacy scheduler store error: {0}")]
    LegacyStore(String),
    #[error("legacy scheduler migration conflict for {job_id}: {field}={value}")]
    MigrationConflict {
        job_id: String,
        field: String,
        value: String,
    },
    #[error("invalid scheduler value: {0}")]
    InvalidValue(String),
    #[error("scheduler schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
}

pub type Result<T> = std::result::Result<T, SchedulerDatabaseError>;

#[derive(Clone)]
pub struct ScheduledTaskDatabase {
    connection: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for ScheduledTaskDatabase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduledTaskDatabase")
            .finish_non_exhaustive()
    }
}

impl ScheduledTaskDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MILLIS))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        ensure_schema(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn import_legacy_store(&self, store: &ScheduledTaskStore) -> Result<usize> {
        if self.migration_applied(LEGACY_JSON_MIGRATION)? {
            return Ok(0);
        }
        let snapshot = LegacySchedulerSnapshot::read_from(store)?;
        self.import_legacy_snapshot_once(&snapshot, Utc::now())
    }

    pub fn import_legacy_snapshot_once(
        &self,
        snapshot: &LegacySchedulerSnapshot,
        applied_at: DateTime<Utc>,
    ) -> Result<usize> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if migration_applied_tx(&transaction, LEGACY_JSON_MIGRATION)? {
            transaction.commit()?;
            return Ok(0);
        }

        let mut imported = 0;
        for definition in &snapshot.definitions {
            let triggers = snapshot
                .triggers
                .get(definition.id())
                .map(Vec::as_slice)
                .unwrap_or_default();
            imported += import_legacy_definition_tx(&transaction, definition, triggers)?;
        }
        transaction.execute(
            "INSERT INTO scheduler_migrations(name, applied_at, details_json)
             VALUES (?1, ?2, ?3)",
            params![
                LEGACY_JSON_MIGRATION,
                timestamp_millis(applied_at),
                serde_json::json!({"definitionsImported": imported}).to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(imported)
    }

    pub fn create_job(
        &self,
        definition: &ScheduledTaskDefinition,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<ScheduledJobRecord> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO scheduled_jobs (
                 id, project_id, enabled, definition_json, revision, next_run_at,
                 created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7)",
            params![
                definition.id(),
                definition.project_id,
                definition.enabled,
                serde_json::to_string(definition)?,
                next_run_at.map(timestamp_millis),
                timestamp_millis(definition.created_at),
                timestamp_millis(definition.updated_at),
            ],
        )?;
        let record = load_job_record_tx(&transaction, &definition.project_id, definition.id())?
            .ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue(
                    "scheduled job was not available after insertion".to_string(),
                )
            })?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn get_job_definition(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> Result<Option<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        load_job_record(&connection, project_id, job_id)
    }

    pub fn get_job_definition_by_id(&self, job_id: &str) -> Result<Option<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        let project_id = connection
            .query_row(
                "SELECT project_id FROM scheduled_jobs WHERE id = ?1",
                params![job_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        project_id
            .as_deref()
            .map(|project_id| load_job_record(&connection, project_id, job_id))
            .transpose()
            .map(|record| record.flatten())
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn update_job(
        &self,
        definition: &ScheduledTaskDefinition,
        expected_updated_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<UpdateJobResult> {
        if timestamp_millis(definition.updated_at) <= timestamp_millis(expected_updated_at) {
            return Err(SchedulerDatabaseError::InvalidValue(
                "scheduled job updated_at must advance by at least one millisecond".to_string(),
            ));
        }
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_jobs
             SET project_id = ?2,
                 enabled = ?3,
                 definition_json = ?4,
                 next_run_at = ?5,
                 revision = revision + 1,
                 created_at = ?6,
                 updated_at = ?7
             WHERE id = ?1 AND project_id = ?2 AND updated_at = ?8",
            params![
                definition.id(),
                definition.project_id,
                definition.enabled,
                serde_json::to_string(definition)?,
                next_run_at.map(timestamp_millis),
                timestamp_millis(definition.created_at),
                timestamp_millis(definition.updated_at),
                timestamp_millis(expected_updated_at),
            ],
        )?;
        let current = load_job_record_tx(&transaction, &definition.project_id, definition.id())?;
        let result = match (updated, current) {
            (1, Some(record)) => UpdateJobResult::Updated(record),
            (0, Some(record)) => UpdateJobResult::Conflict(record),
            (0, None) => UpdateJobResult::NotFound,
            _ => {
                return Err(SchedulerDatabaseError::InvalidValue(
                    "optimistic scheduled job update affected an unexpected row count".to_string(),
                ));
            }
        };
        transaction.commit()?;
        Ok(result)
    }

    pub fn set_job_enabled(
        &self,
        project_id: &str,
        job_id: &str,
        expected_updated_at: DateTime<Utc>,
        enabled: bool,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<UpdateJobResult> {
        let Some(current) = self.get_job_definition(project_id, job_id)? else {
            return Ok(UpdateJobResult::NotFound);
        };
        if current.definition.updated_at != expected_updated_at {
            return Ok(UpdateJobResult::Conflict(current));
        }
        let mut definition = current.definition;
        definition.enabled = enabled;
        let now = Utc::now();
        definition.updated_at = if now > expected_updated_at {
            now
        } else {
            expected_updated_at + chrono::Duration::milliseconds(1)
        };
        self.update_job(&definition, expected_updated_at, next_run_at)
    }

    pub fn list_enabled_jobs(&self) -> Result<Vec<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE enabled = 1 AND definition_json IS NOT NULL
             ORDER BY next_run_at ASC, created_at ASC, id ASC",
        )?;
        let rows = statement.query_map([], map_job_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn list_recoverable_jobs_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<RecoverableScheduledJob>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT job.definition_json,
                    job.revision,
                    job.next_run_at,
                    EXISTS(
                        SELECT 1 FROM scheduled_occurrences AS occurrence
                        WHERE occurrence.job_id = job.id
                          AND occurrence.status IN ('pending', 'retrying')
                    ),
                    (
                        SELECT MIN(occurrence.lease_until)
                        FROM scheduled_occurrences AS occurrence
                        WHERE occurrence.job_id = job.id
                          AND occurrence.status = 'running'
                    )
             FROM scheduled_jobs AS job
             WHERE job.project_id = ?1
               AND job.definition_json IS NOT NULL
               AND (
                   job.enabled = 1
                   OR EXISTS(
                       SELECT 1 FROM scheduled_occurrences AS occurrence
                       WHERE occurrence.job_id = job.id
                         AND occurrence.status IN ('pending', 'retrying', 'running')
                   )
               )
             ORDER BY job.next_run_at ASC, job.created_at ASC, job.id ASC",
        )?;
        let rows = statement.query_map(params![project_id], map_recoverable_job)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn get_recoverable_job_for_project(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> Result<Option<RecoverableScheduledJob>> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT job.definition_json,
                        job.revision,
                        job.next_run_at,
                        EXISTS(
                            SELECT 1 FROM scheduled_occurrences AS occurrence
                            WHERE occurrence.job_id = job.id
                              AND occurrence.status IN ('pending', 'retrying')
                        ),
                        (
                            SELECT MIN(occurrence.lease_until)
                            FROM scheduled_occurrences AS occurrence
                            WHERE occurrence.job_id = job.id
                              AND occurrence.status = 'running'
                        )
                 FROM scheduled_jobs AS job
                 WHERE job.project_id = ?1
                   AND job.id = ?2
                   AND job.definition_json IS NOT NULL
                   AND (
                       job.enabled = 1
                       OR EXISTS(
                           SELECT 1 FROM scheduled_occurrences AS occurrence
                           WHERE occurrence.job_id = job.id
                             AND occurrence.status IN ('pending', 'retrying', 'running')
                       )
                   )",
                params![project_id, job_id],
                map_recoverable_job,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn enabled_job_count(&self) -> Result<usize> {
        let connection = self.lock_connection()?;
        let count = connection.query_row(
            "SELECT COUNT(*) FROM scheduled_jobs
             WHERE enabled = 1 AND definition_json IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        count.try_into().map_err(|_| {
            SchedulerDatabaseError::InvalidValue(format!(
                "enabled scheduled job count is out of range: {count}"
            ))
        })
    }

    pub fn save_job_definition(&self, definition: &ScheduledTaskDefinition) -> Result<()> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let definition_json = serde_json::to_string(definition)?;
        transaction.execute(
            "INSERT INTO scheduled_jobs (
                 id, project_id, enabled, definition_json, revision, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 project_id = excluded.project_id,
                 enabled = excluded.enabled,
                 definition_json = excluded.definition_json,
                 created_at = excluded.created_at,
                 updated_at = excluded.updated_at,
                 revision = scheduled_jobs.revision + 1",
            params![
                definition.id(),
                definition.project_id,
                definition.enabled,
                definition_json,
                timestamp_millis(definition.created_at),
                timestamp_millis(definition.updated_at),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_job(&self, project_id: &str, job_id: &str) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM scheduled_occurrences
             WHERE job_id = ?2
               AND EXISTS (
                   SELECT 1 FROM scheduled_jobs
                   WHERE id = ?2 AND project_id = ?1
               )",
            params![project_id, job_id],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM scheduled_jobs WHERE project_id = ?1 AND id = ?2",
            params![project_id, job_id],
        )?;
        transaction.commit()?;
        Ok(deleted == 1)
    }

    pub fn copy_project_from(&self, source: &Self, project_id: &str) -> Result<usize> {
        self.import_legacy_database_once(source, project_id, Utc::now())
    }

    pub fn import_legacy_database_path_once(
        &self,
        source_path: impl AsRef<Path>,
        project_id: &str,
        applied_at: DateTime<Utc>,
    ) -> Result<usize> {
        if self.migration_applied(LEGACY_SHARED_DB_MIGRATION)? {
            return Ok(0);
        }
        let source_path = source_path.as_ref();
        if !source_path.is_file() {
            return Ok(0);
        }
        let source = Self::open(source_path)?;
        self.import_legacy_database_once(&source, project_id, applied_at)
    }

    pub fn import_legacy_database_once(
        &self,
        source: &ScheduledTaskDatabase,
        project_id: &str,
        applied_at: DateTime<Utc>,
    ) -> Result<usize> {
        if Arc::ptr_eq(&self.connection, &source.connection) {
            return Ok(0);
        }
        if self.migration_applied(LEGACY_SHARED_DB_MIGRATION)? {
            return Ok(0);
        }

        let jobs = source.list_job_records_for_project(project_id)?;
        let mut source_occurrences = BTreeMap::new();
        for job in &jobs {
            source_occurrences.insert(
                job.definition.id().to_string(),
                source.list_all_occurrences(job.definition.id())?,
            );
        }

        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if migration_applied_tx(&transaction, LEGACY_SHARED_DB_MIGRATION)? {
            transaction.commit()?;
            return Ok(0);
        }
        let mut imported = 0;
        for job in &jobs {
            imported += import_database_job_tx(&transaction, job)?;
            if let Some(occurrences) = source_occurrences.get(job.definition.id()) {
                for occurrence in occurrences {
                    import_database_occurrence_tx(&transaction, occurrence)?;
                }
            }
        }
        transaction.execute(
            "INSERT INTO scheduler_migrations(name, applied_at, details_json)
             VALUES (?1, ?2, ?3)",
            params![
                LEGACY_SHARED_DB_MIGRATION,
                timestamp_millis(applied_at),
                serde_json::json!({
                    "projectId": project_id,
                    "definitionsImported": imported,
                })
                .to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(imported)
    }

    pub fn list_job_definitions(&self) -> Result<Vec<ScheduledTaskDefinition>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT definition_json
             FROM scheduled_jobs
             WHERE definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut definitions = Vec::new();
        for row in rows {
            let definition_json = row?;
            definitions.push(serde_json::from_str(&definition_json)?);
        }
        Ok(definitions)
    }

    pub fn scan_job_definitions(&self) -> Result<ScheduledJobDefinitionScan> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT definition_json
             FROM scheduled_jobs
             WHERE definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut definitions = Vec::new();
        let mut invalid_count = 0;
        for row in rows {
            match row.and_then(|definition_json| {
                serde_json::from_str(&definition_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            }) {
                Ok(definition) => definitions.push(definition),
                Err(_) => invalid_count += 1,
            }
        }
        Ok(ScheduledJobDefinitionScan {
            definitions,
            invalid_count,
        })
    }

    pub fn list_job_definitions_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<ScheduledTaskDefinition>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT definition_json
             FROM scheduled_jobs
             WHERE project_id = ?1 AND definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(params![project_id], |row| row.get::<_, String>(0))?;
        let mut definitions = Vec::new();
        for row in rows {
            let definition_json = row?;
            definitions.push(serde_json::from_str(&definition_json)?);
        }
        Ok(definitions)
    }

    fn list_job_records_for_project(&self, project_id: &str) -> Result<Vec<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE project_id = ?1 AND definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(params![project_id], map_job_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn create_or_get_occurrence_for_existing_job(
        &self,
        project_id: &str,
        job_id: &str,
        scheduled_at: DateTime<Utc>,
        trigger_kind: OccurrenceTriggerKind,
    ) -> Result<Option<ScheduledOccurrence>> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !job_exists_tx(&transaction, project_id, job_id)? {
            transaction.commit()?;
            return Ok(None);
        }
        let occurrence = insert_or_get_occurrence_tx(
            &transaction,
            job_id,
            scheduled_at,
            trigger_kind,
            Utc::now(),
        )?;
        transaction.commit()?;
        Ok(Some(occurrence))
    }

    #[cfg(test)]
    pub fn create_or_get_occurrence(
        &self,
        job_id: &str,
        scheduled_at: DateTime<Utc>,
        trigger_kind: OccurrenceTriggerKind,
    ) -> Result<ScheduledOccurrence> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        ensure_job_row(&transaction, job_id, now)?;
        let occurrence =
            insert_or_get_occurrence_tx(&transaction, job_id, scheduled_at, trigger_kind, now)?;
        transaction.commit()?;
        Ok(occurrence)
    }

    pub fn materialize_due_occurrence(
        &self,
        project_id: &str,
        job_id: &str,
        expected_revision: i64,
        now: DateTime<Utc>,
    ) -> Result<DueMaterialization> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(current) = load_job_record_tx(&transaction, project_id, job_id)? else {
            transaction.commit()?;
            return Ok(DueMaterialization::Stale);
        };
        if current.revision != expected_revision {
            transaction.commit()?;
            return Ok(DueMaterialization::Stale);
        }
        if !current.definition.enabled {
            transaction.commit()?;
            return Ok(DueMaterialization::Disabled);
        }
        let Some(deadline) = current.next_run_at else {
            transaction.commit()?;
            return Ok(DueMaterialization::NotDue);
        };
        if deadline > now {
            transaction.commit()?;
            return Ok(DueMaterialization::NotDue);
        }

        transaction.execute(
            "INSERT OR IGNORE INTO scheduled_occurrences (
                 id, job_id, scheduled_at, trigger_kind, status, attempt,
                 created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'scheduled', 'pending', 0, ?4, ?4)",
            params![
                format!("occurrence-{}", Uuid::new_v4()),
                job_id,
                timestamp_millis(deadline),
                timestamp_millis(now),
            ],
        )?;
        let occurrence = load_occurrence_by_key(
            &transaction,
            job_id,
            deadline,
            OccurrenceTriggerKind::Scheduled,
        )?
        .ok_or_else(|| {
            SchedulerDatabaseError::InvalidValue(
                "due occurrence was not available after insertion".to_string(),
            )
        })?;
        let following_deadline = current.definition.schedule.next_occurrence_after(deadline);
        let updated = transaction.execute(
            "UPDATE scheduled_jobs
             SET next_run_at = ?4, revision = revision + 1
             WHERE project_id = ?1 AND id = ?2 AND revision = ?3 AND enabled = 1",
            params![
                project_id,
                job_id,
                expected_revision,
                following_deadline.map(timestamp_millis),
            ],
        )?;
        if updated != 1 {
            return Err(SchedulerDatabaseError::InvalidValue(
                "due job changed while holding the scheduler write transaction".to_string(),
            ));
        }
        let job = load_job_record_tx(&transaction, project_id, job_id)?.ok_or_else(|| {
            SchedulerDatabaseError::InvalidValue(
                "scheduled job disappeared after due materialization".to_string(),
            )
        })?;
        transaction.commit()?;
        Ok(DueMaterialization::Ready { job, occurrence })
    }

    pub fn update_job_runtime_projection(
        &self,
        definition: &ScheduledTaskDefinition,
        expected_revision: i64,
    ) -> Result<UpdateJobResult> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(current) =
            load_job_record_tx(&transaction, &definition.project_id, definition.id())?
        else {
            transaction.commit()?;
            return Ok(UpdateJobResult::NotFound);
        };
        if current.revision != expected_revision {
            transaction.commit()?;
            return Ok(UpdateJobResult::Conflict(current));
        }
        let minimum_updated_at = timestamp_millis(current.definition.updated_at)
            .checked_add(1)
            .ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue(
                    "scheduled job updated_at cannot be advanced".to_string(),
                )
            })?;
        let normalized_updated_at = timestamp_millis(definition.updated_at).max(minimum_updated_at);
        let mut normalized_definition = definition.clone();
        normalized_definition.updated_at = from_timestamp_millis(normalized_updated_at)
            .map_err(SchedulerDatabaseError::InvalidValue)?;
        let updated = transaction.execute(
            "UPDATE scheduled_jobs
             SET project_id = ?2,
                 enabled = ?3,
                 definition_json = ?4,
                 created_at = ?5,
                 updated_at = ?6,
                 revision = revision + 1
             WHERE id = ?1 AND project_id = ?2 AND revision = ?7",
            params![
                definition.id(),
                definition.project_id,
                definition.enabled,
                serde_json::to_string(&normalized_definition)?,
                timestamp_millis(definition.created_at),
                normalized_updated_at,
                expected_revision,
            ],
        )?;
        if updated != 1 {
            return Err(SchedulerDatabaseError::InvalidValue(
                "runtime projection changed while holding the scheduler write transaction"
                    .to_string(),
            ));
        }
        let updated = load_job_record_tx(&transaction, &definition.project_id, definition.id())?
            .ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue(
                    "scheduled job disappeared after runtime projection update".to_string(),
                )
            })?;
        transaction.commit()?;
        Ok(UpdateJobResult::Updated(updated))
    }

    pub fn get_occurrence(&self, id: &str) -> Result<Option<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        load_occurrence_by_id(&connection, id)
    }

    pub fn claim_occurrence(
        &self,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimResult> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((status, current_owner, current_lease)) =
            occurrence_claim_state(&transaction, id)?
        else {
            transaction.commit()?;
            return Ok(ClaimResult::NotFound);
        };

        let now_millis = timestamp_millis(now);
        let current_lease = current_lease
            .map(from_timestamp_millis)
            .transpose()
            .map_err(SchedulerDatabaseError::InvalidValue)?;
        let status = status
            .parse::<OccurrenceStatus>()
            .map_err(|error| SchedulerDatabaseError::InvalidValue(error))?;

        if status.is_terminal() {
            transaction.commit()?;
            return Ok(ClaimResult::Busy);
        }

        if status == OccurrenceStatus::Running {
            let lease_active = current_lease.is_some_and(|lease| lease > now);
            if lease_active {
                let result = if current_owner.as_deref() == Some(owner_id) {
                    ClaimResult::AlreadyOwned
                } else {
                    ClaimResult::Busy
                };
                transaction.commit()?;
                return Ok(result);
            }
        }

        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET status = 'running',
                 attempt = attempt + 1,
                 owner_id = ?2,
                 lease_until = ?3,
                 heartbeat_at = ?4,
                 started_at = COALESCE(started_at, ?4),
                 finished_at = NULL,
                 error_code = NULL,
                 error_params = NULL,
                 updated_at = ?4
             WHERE id = ?1
               AND (
                   status IN ('pending', 'retrying')
                   OR (status = 'running' AND lease_until IS NOT NULL AND lease_until <= ?5)
               )",
            params![
                id,
                owner_id,
                timestamp_millis(lease_until),
                now_millis,
                now_millis,
            ],
        )?;

        if updated != 1 {
            let result = if current_owner.as_deref() == Some(owner_id) {
                ClaimResult::AlreadyOwned
            } else {
                ClaimResult::Busy
            };
            transaction.commit()?;
            return Ok(result);
        }

        let occurrence = load_occurrence_by_id_tx(&transaction, id)?.ok_or_else(|| {
            SchedulerDatabaseError::InvalidValue("claimed occurrence disappeared".to_string())
        })?;
        transaction.commit()?;
        Ok(ClaimResult::Claimed(occurrence))
    }

    pub fn resume_attention_occurrence(
        &self,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimResult> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((status, _, _)) = occurrence_claim_state(&transaction, id)? else {
            transaction.commit()?;
            return Ok(ClaimResult::NotFound);
        };
        if status != OccurrenceStatus::AttentionRequired.to_string() {
            transaction.commit()?;
            return Ok(ClaimResult::Busy);
        }
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET status = 'running',
                 attempt = attempt + 1,
                 owner_id = ?2,
                 lease_until = ?3,
                 heartbeat_at = ?4,
                 started_at = COALESCE(started_at, ?4),
                 finished_at = NULL,
                 error_code = NULL,
                 error_params = NULL,
                 updated_at = ?4
             WHERE id = ?1 AND status = 'attention_required'",
            params![
                id,
                owner_id,
                timestamp_millis(lease_until),
                timestamp_millis(now),
            ],
        )?;
        if updated != 1 {
            transaction.commit()?;
            return Ok(ClaimResult::Busy);
        }
        let occurrence = load_occurrence_by_id_tx(&transaction, id)?.ok_or_else(|| {
            SchedulerDatabaseError::InvalidValue(
                "resumed attention occurrence disappeared".to_string(),
            )
        })?;
        transaction.commit()?;
        Ok(ClaimResult::Claimed(occurrence))
    }

    pub fn find_attention_occurrence_by_links(
        &self,
        task_id: &str,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> Result<Option<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                        owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                        error_code, error_params, started_at, finished_at, created_at, updated_at
                 FROM scheduled_occurrences
                 WHERE status = 'attention_required'
                   AND task_id = ?1
                   AND run_id = ?2
                   AND round_id = ?3
                   AND attempt_id = ?4
                 ORDER BY updated_at DESC
                 LIMIT 1",
                params![task_id, run_id, round_id, attempt_id],
                map_occurrence,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn renew_lease(
        &self,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET lease_until = ?3, heartbeat_at = ?4, updated_at = ?4
             WHERE id = ?1
               AND owner_id = ?2
               AND status = 'running'
               AND lease_until IS NOT NULL
               AND lease_until > ?4",
            params![
                id,
                owner_id,
                timestamp_millis(lease_until),
                timestamp_millis(now),
            ],
        )?;
        transaction.commit()?;
        Ok(updated == 1)
    }

    pub fn accept_occurrence_links(
        &self,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        links: &OccurrenceLinks,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET task_id = COALESCE(task_id, ?4),
                 run_id = COALESCE(run_id, ?5),
                 round_id = COALESCE(round_id, ?6),
                 attempt_id = COALESCE(attempt_id, ?7),
                 updated_at = ?3
             WHERE id = ?1
               AND owner_id = ?2
               AND status = 'running'
               AND lease_until IS NOT NULL
               AND lease_until > ?3
               AND (task_id IS NULL OR ?4 IS NULL OR task_id = ?4)
               AND (run_id IS NULL OR ?5 IS NULL OR run_id = ?5)
               AND (round_id IS NULL OR ?6 IS NULL OR round_id = ?6)
               AND (attempt_id IS NULL OR ?7 IS NULL OR attempt_id = ?7)",
            params![
                id,
                owner_id,
                timestamp_millis(now),
                links.task_id.as_deref(),
                links.run_id.as_deref(),
                links.round_id.as_deref(),
                links.attempt_id.as_deref(),
            ],
        )?;
        transaction.commit()?;
        Ok(updated == 1)
    }

    pub fn release_owned_occurrence_for_retry(
        &self,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET status = 'retrying',
                 owner_id = NULL,
                 lease_until = NULL,
                 heartbeat_at = NULL,
                 error_code = 'SCHEDULED_LEASE_LOST',
                 error_params = NULL,
                 finished_at = NULL,
                 updated_at = ?3
             WHERE id = ?1 AND owner_id = ?2 AND status = 'running'",
            params![id, owner_id, timestamp_millis(now)],
        )?;
        transaction.commit()?;
        Ok(updated == 1)
    }

    pub fn finish_occurrence(
        &self,
        id: &str,
        owner_id: &str,
        status: OccurrenceStatus,
        links: Option<OccurrenceLinks>,
        error: Option<ScheduledError>,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        let finished_at = status.is_terminal().then_some(timestamp_millis(now));
        let links = links.unwrap_or_default();
        let error_code = error.as_ref().map(|value| value.code.to_string());
        let error_params = error
            .as_ref()
            .and_then(|value| value.params.as_ref())
            .map(serde_json::to_string)
            .transpose()?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET status = ?3,
                 owner_id = NULL,
                 lease_until = NULL,
                 heartbeat_at = NULL,
                  task_id = COALESCE(?4, task_id),
                  run_id = COALESCE(?5, run_id),
                  round_id = COALESCE(?6, round_id),
                  attempt_id = COALESCE(?7, attempt_id),
                 error_code = ?8,
                 error_params = ?9,
                 finished_at = ?10,
                 updated_at = ?11
             WHERE id = ?1
               AND owner_id = ?2
               AND status = 'running'
               AND (lease_until IS NULL OR lease_until >= ?11)",
            params![
                id,
                owner_id,
                status.to_string(),
                links.task_id.as_deref(),
                links.run_id.as_deref(),
                links.round_id.as_deref(),
                links.attempt_id.as_deref(),
                error_code,
                error_params,
                finished_at,
                timestamp_millis(now),
            ],
        )?;
        transaction.commit()?;
        Ok(updated == 1)
    }

    pub fn recover_expired(&self, now: DateTime<Utc>) -> Result<usize> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET status = 'retrying',
                 owner_id = NULL,
                 lease_until = NULL,
                 heartbeat_at = NULL,
                 error_code = 'SCHEDULED_LEASE_LOST',
                 error_params = NULL,
                 updated_at = ?1
             WHERE status = 'running' AND (lease_until IS NULL OR lease_until <= ?1)",
            params![timestamp_millis(now)],
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn mark_missed_for_existing_job(
        &self,
        project_id: &str,
        job_id: &str,
        scheduled_at: DateTime<Utc>,
    ) -> Result<Option<bool>> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !job_exists_tx(&transaction, project_id, job_id)? {
            transaction.commit()?;
            return Ok(None);
        }
        let updated = mark_missed_tx(&transaction, job_id, scheduled_at, Utc::now())?;
        transaction.commit()?;
        Ok(Some(updated))
    }

    #[cfg(test)]
    pub fn mark_missed(&self, job_id: &str, scheduled_at: DateTime<Utc>) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        ensure_job_row(&transaction, job_id, now)?;
        let updated = mark_missed_tx(&transaction, job_id, scheduled_at, now)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn list_occurrences(&self, job_id: &str, limit: usize) -> Result<Vec<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences
             WHERE job_id = ?1
             ORDER BY scheduled_at DESC, created_at DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![job_id, limit as i64], map_occurrence)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn oldest_runnable_occurrence(&self, job_id: &str) -> Result<Option<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                        owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                        error_code, error_params, started_at, finished_at, created_at, updated_at
                 FROM scheduled_occurrences
                 WHERE job_id = ?1
                   AND status IN ('pending', 'retrying')
                 ORDER BY scheduled_at ASC, created_at ASC
                 LIMIT 1",
                params![job_id],
                map_occurrence,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn cleanup_terminal_occurrences(
        &self,
        cutoff: DateTime<Utc>,
        batch_size: usize,
        protected_run_ids: &HashSet<String>,
    ) -> Result<RetentionResult> {
        if batch_size == 0 {
            return Err(SchedulerDatabaseError::InvalidValue(
                "retention batch size must be greater than zero".to_string(),
            ));
        }
        let batch_size = i64::try_from(batch_size).map_err(|_| {
            SchedulerDatabaseError::InvalidValue(format!(
                "retention batch size is out of range: {batch_size}"
            ))
        })?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS scheduler_protected_runs (
                 run_id TEXT PRIMARY KEY
             ) WITHOUT ROWID;
             DELETE FROM scheduler_protected_runs;",
        )?;
        for run_id in protected_run_ids {
            transaction.execute(
                "INSERT INTO scheduler_protected_runs(run_id) VALUES (?1)",
                params![run_id],
            )?;
        }
        let deleted = transaction.execute(
            "DELETE FROM scheduled_occurrences
             WHERE id IN (
                 SELECT occurrence.id
                 FROM scheduled_occurrences AS occurrence
                 WHERE occurrence.status IN ('succeeded', 'failed', 'skipped', 'missed')
                   AND occurrence.finished_at IS NOT NULL
                   AND occurrence.finished_at < ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM scheduler_protected_runs AS protected
                       WHERE protected.run_id = occurrence.run_id
                   )
                 ORDER BY occurrence.finished_at ASC, occurrence.id ASC
                 LIMIT ?2
             )",
            params![timestamp_millis(cutoff), batch_size],
        )?;
        let has_more = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM scheduled_occurrences AS occurrence
                 WHERE occurrence.status IN ('succeeded', 'failed', 'skipped', 'missed')
                   AND occurrence.finished_at IS NOT NULL
                   AND occurrence.finished_at < ?1
                   AND NOT EXISTS (
                       SELECT 1 FROM scheduler_protected_runs AS protected
                       WHERE protected.run_id = occurrence.run_id
                   )
             )",
            params![timestamp_millis(cutoff)],
            |row| row.get(0),
        )?;
        transaction.execute("DELETE FROM scheduler_protected_runs", [])?;
        transaction.commit()?;
        Ok(RetentionResult { deleted, has_more })
    }

    fn list_all_occurrences(&self, job_id: &str) -> Result<Vec<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences
             WHERE job_id = ?1
             ORDER BY scheduled_at ASC, created_at ASC",
        )?;
        let rows = statement.query_map(params![job_id], map_occurrence)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn schema_version(&self) -> Result<i64> {
        let connection = self.lock_connection()?;
        Ok(
            connection.query_row("SELECT version FROM scheduler_schema LIMIT 1", [], |row| {
                row.get(0)
            })?,
        )
    }

    fn migration_applied(&self, name: &str) -> Result<bool> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM scheduler_migrations WHERE name = ?1)",
                params![name],
                |row| row.get(0),
            )
            .map_err(SchedulerDatabaseError::from)
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| {
            SchedulerDatabaseError::InvalidValue("scheduler database lock poisoned".to_string())
        })
    }
}

fn migration_applied_tx(transaction: &Transaction<'_>, name: &str) -> Result<bool> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM scheduler_migrations WHERE name = ?1)",
            params![name],
            |row| row.get(0),
        )
        .map_err(SchedulerDatabaseError::from)
}

fn import_legacy_definition_tx(
    transaction: &Transaction<'_>,
    definition: &ScheduledTaskDefinition,
    triggers: &[ScheduledTriggerRecord],
) -> Result<usize> {
    let definition_json = serde_json::to_string(definition)?;
    let next_run_at = derived_next_run_at(definition);
    let existing_definition = transaction
        .query_row(
            "SELECT definition_json, next_run_at FROM scheduled_jobs WHERE id = ?1",
            params![definition.id()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                ))
            },
        )
        .optional()?;
    let imported = match existing_definition {
        None => {
            transaction.execute(
                "INSERT INTO scheduled_jobs (
                     id, project_id, enabled, definition_json, revision, next_run_at,
                     created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7)",
                params![
                    definition.id(),
                    definition.project_id,
                    definition.enabled,
                    definition_json,
                    next_run_at.map(timestamp_millis),
                    timestamp_millis(definition.created_at),
                    timestamp_millis(definition.updated_at),
                ],
            )?;
            1
        }
        Some((Some(existing_json), _)) if existing_json != definition_json => {
            return Err(SchedulerDatabaseError::MigrationConflict {
                job_id: definition.id().to_string(),
                field: "definition_json".to_string(),
                value: existing_json,
            });
        }
        Some((None, _)) => {
            transaction.execute(
                "UPDATE scheduled_jobs
                 SET project_id = ?2, enabled = ?3, definition_json = ?4,
                     next_run_at = ?5, created_at = ?6, updated_at = ?7
                 WHERE id = ?1",
                params![
                    definition.id(),
                    definition.project_id,
                    definition.enabled,
                    definition_json,
                    next_run_at.map(timestamp_millis),
                    timestamp_millis(definition.created_at),
                    timestamp_millis(definition.updated_at),
                ],
            )?;
            1
        }
        Some((Some(_), None)) => {
            if let Some(next_run_at) = next_run_at {
                transaction.execute(
                    "UPDATE scheduled_jobs SET next_run_at = ?2 WHERE id = ?1",
                    params![definition.id(), timestamp_millis(next_run_at)],
                )?;
            }
            0
        }
        Some((Some(_), Some(_))) => 0,
    };

    for trigger in triggers {
        if trigger.scheduled_task_id != definition.id() {
            return Err(SchedulerDatabaseError::MigrationConflict {
                job_id: definition.id().to_string(),
                field: "scheduled_task_id".to_string(),
                value: trigger.scheduled_task_id.clone(),
            });
        }
        let occurrence_id = legacy_occurrence_id(definition.id(), &trigger.id);
        let status = legacy_trigger_status(&trigger.status)?;
        let existing_by_id = load_occurrence_by_id_tx(transaction, &occurrence_id)?;
        let existing_by_key = load_occurrence_by_key(
            transaction,
            definition.id(),
            trigger.scheduled_at,
            OccurrenceTriggerKind::Scheduled,
        )?;

        if let Some(existing) = existing_by_id {
            if !legacy_occurrence_matches(&existing, &occurrence_id, trigger, status) {
                return Err(SchedulerDatabaseError::MigrationConflict {
                    job_id: definition.id().to_string(),
                    field: "occurrence_id".to_string(),
                    value: occurrence_id,
                });
            }
            continue;
        }
        if existing_by_key.is_some() {
            return Err(SchedulerDatabaseError::MigrationConflict {
                job_id: definition.id().to_string(),
                field: "scheduled_at".to_string(),
                value: trigger.scheduled_at.to_rfc3339(),
            });
        }

        let finished_at = status
            .is_terminal()
            .then_some(timestamp_millis(trigger.updated_at));
        transaction.execute(
            "INSERT INTO scheduled_occurrences (
                 id, job_id, scheduled_at, trigger_kind, status, attempt,
                 task_id, run_id, finished_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'scheduled', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                occurrence_id,
                definition.id(),
                timestamp_millis(trigger.scheduled_at),
                status.to_string(),
                trigger.attempts,
                trigger.task_id,
                trigger.run_id,
                finished_at,
                timestamp_millis(trigger.created_at),
                timestamp_millis(trigger.updated_at),
            ],
        )?;
    }

    Ok(imported)
}

fn import_database_job_tx(
    transaction: &Transaction<'_>,
    job: &ScheduledJobRecord,
) -> Result<usize> {
    let mut normalized_job = job.clone();
    if normalized_job.next_run_at.is_none() {
        normalized_job.next_run_at = derived_next_run_at(&normalized_job.definition);
    }
    let existing = transaction
        .query_row(
            "SELECT definition_json, revision, next_run_at
             FROM scheduled_jobs WHERE id = ?1 AND definition_json IS NOT NULL",
            params![job.definition.id()],
            map_job_record,
        )
        .optional()?;
    match existing {
        None => {
            transaction.execute(
                "INSERT INTO scheduled_jobs (
                     id, project_id, enabled, definition_json, revision, next_run_at,
                     created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    normalized_job.definition.id(),
                    normalized_job.definition.project_id,
                    normalized_job.definition.enabled,
                    serde_json::to_string(&normalized_job.definition)?,
                    normalized_job.revision,
                    normalized_job.next_run_at.map(timestamp_millis),
                    timestamp_millis(normalized_job.definition.created_at),
                    timestamp_millis(normalized_job.definition.updated_at),
                ],
            )?;
            Ok(1)
        }
        Some(existing) if existing == normalized_job => Ok(0),
        Some(existing) => Err(SchedulerDatabaseError::MigrationConflict {
            job_id: job.definition.id().to_string(),
            field: "definition_json".to_string(),
            value: serde_json::to_string(&existing.definition)?,
        }),
    }
}

fn import_database_occurrence_tx(
    transaction: &Transaction<'_>,
    occurrence: &ScheduledOccurrence,
) -> Result<()> {
    if let Some(existing) = load_occurrence_by_id_tx(transaction, &occurrence.id)? {
        if existing == *occurrence {
            return Ok(());
        }
        return Err(SchedulerDatabaseError::MigrationConflict {
            job_id: occurrence.job_id.clone(),
            field: "occurrence_id".to_string(),
            value: occurrence.id.clone(),
        });
    }
    if load_occurrence_by_key(
        transaction,
        &occurrence.job_id,
        occurrence.scheduled_at,
        occurrence.trigger_kind,
    )?
    .is_some()
    {
        return Err(SchedulerDatabaseError::MigrationConflict {
            job_id: occurrence.job_id.clone(),
            field: "scheduled_at".to_string(),
            value: occurrence.scheduled_at.to_rfc3339(),
        });
    }
    let error_params = occurrence
        .error_params
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    transaction.execute(
        "INSERT INTO scheduled_occurrences (
             id, job_id, scheduled_at, trigger_kind, status, attempt,
             owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
             error_code, error_params, started_at, finished_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            occurrence.id,
            occurrence.job_id,
            timestamp_millis(occurrence.scheduled_at),
            occurrence.trigger_kind.to_string(),
            occurrence.status.to_string(),
            occurrence.attempt,
            occurrence.owner_id,
            occurrence.lease_until.map(timestamp_millis),
            occurrence.heartbeat_at.map(timestamp_millis),
            occurrence.task_id,
            occurrence.run_id,
            occurrence.round_id,
            occurrence.attempt_id,
            occurrence.error_code.map(|value| value.to_string()),
            error_params,
            occurrence.started_at.map(timestamp_millis),
            occurrence.finished_at.map(timestamp_millis),
            timestamp_millis(occurrence.created_at),
            timestamp_millis(occurrence.updated_at),
        ],
    )?;
    Ok(())
}

fn ensure_schema(connection: &mut Connection) -> Result<()> {
    let schema_exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'scheduler_schema'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if schema_exists {
        let version = connection.query_row(
            "SELECT version FROM scheduler_schema WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        if version > SCHEMA_VERSION {
            return Err(SchedulerDatabaseError::UnsupportedSchemaVersion {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS scheduler_schema (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             version INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO scheduler_schema(id, version) VALUES (1, 1);

         CREATE TABLE IF NOT EXISTS scheduled_jobs (
             id TEXT PRIMARY KEY,
             project_id TEXT,
             enabled INTEGER NOT NULL DEFAULT 1,
             definition_json TEXT,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS scheduled_occurrences (
             id TEXT PRIMARY KEY,
             job_id TEXT NOT NULL,
             scheduled_at INTEGER NOT NULL,
             trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('scheduled', 'manual')),
             status TEXT NOT NULL CHECK (status IN (
                 'pending', 'running', 'retrying', 'succeeded', 'failed',
                 'skipped', 'missed', 'attention_required'
             )),
             attempt INTEGER NOT NULL DEFAULT 0,
             owner_id TEXT,
             lease_until INTEGER,
             heartbeat_at INTEGER,
             task_id TEXT,
             run_id TEXT,
             round_id TEXT,
             attempt_id TEXT,
             error_code TEXT,
             error_params TEXT,
             started_at INTEGER,
             finished_at INTEGER,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             UNIQUE(job_id, scheduled_at, trigger_kind)
         );

         CREATE INDEX IF NOT EXISTS idx_scheduled_occurrences_active
             ON scheduled_occurrences(job_id, scheduled_at)
             WHERE status IN ('pending', 'running', 'retrying');

         CREATE INDEX IF NOT EXISTS idx_scheduled_occurrences_history
             ON scheduled_occurrences(job_id, scheduled_at DESC);",
    )?;

    let version = transaction.query_row(
        "SELECT version FROM scheduler_schema WHERE id = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    match version {
        1 => migrate_schema_v1_to_v2(&transaction)?,
        SCHEMA_VERSION => ensure_schema_v2_objects(&transaction)?,
        other => {
            return Err(SchedulerDatabaseError::InvalidValue(format!(
                "unsupported scheduler schema version: {other}"
            )));
        }
    }
    transaction.commit()?;
    Ok(())
}

fn migrate_schema_v1_to_v2(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "ALTER TABLE scheduled_jobs ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
         ALTER TABLE scheduled_jobs ADD COLUMN next_run_at INTEGER;",
    )?;
    backfill_missing_deadlines(transaction)?;
    ensure_schema_v2_objects(transaction)?;
    transaction.execute(
        "UPDATE scheduler_schema SET version = ?1 WHERE id = 1",
        params![SCHEMA_VERSION],
    )?;
    Ok(())
}

fn backfill_missing_deadlines(transaction: &Transaction<'_>) -> Result<()> {
    let candidates = {
        let mut statement = transaction.prepare(
            "SELECT id, definition_json
             FROM scheduled_jobs
             WHERE definition_json IS NOT NULL AND next_run_at IS NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    for (job_id, definition_json) in candidates {
        let definition = serde_json::from_str::<ScheduledTaskDefinition>(&definition_json)?;
        if let Some(next_run_at) = derived_next_run_at(&definition) {
            transaction.execute(
                "UPDATE scheduled_jobs
                 SET next_run_at = ?2
                 WHERE id = ?1 AND next_run_at IS NULL",
                params![job_id, timestamp_millis(next_run_at)],
            )?;
        }
    }
    Ok(())
}

fn ensure_schema_v2_objects(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS scheduler_migrations (
             name TEXT PRIMARY KEY,
             applied_at INTEGER NOT NULL,
             details_json TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_enabled_deadline
             ON scheduled_jobs(enabled, next_run_at)
             WHERE enabled = 1;",
    )?;
    Ok(())
}

pub fn derived_next_run_at(definition: &ScheduledTaskDefinition) -> Option<DateTime<Utc>> {
    if !definition.enabled {
        return None;
    }
    let baseline = definition.last_trigger_at.unwrap_or_else(|| {
        definition
            .created_at
            .checked_sub_signed(chrono::Duration::seconds(1))
            .unwrap_or(definition.created_at)
    });
    definition.schedule.next_occurrence_after(baseline)
}

fn legacy_occurrence_id(job_id: &str, trigger_id: &str) -> String {
    format!("legacy:{job_id}:{trigger_id}")
}

fn legacy_trigger_status(value: &str) -> Result<OccurrenceStatus> {
    match value {
        "completed" | "succeeded" => Ok(OccurrenceStatus::Succeeded),
        "failed" => Ok(OccurrenceStatus::Failed),
        "skipped" => Ok(OccurrenceStatus::Skipped),
        "missed" => Ok(OccurrenceStatus::Missed),
        "pending" => Ok(OccurrenceStatus::Pending),
        "running" => Ok(OccurrenceStatus::Running),
        "retrying" => Ok(OccurrenceStatus::Retrying),
        "attention_required" => Ok(OccurrenceStatus::AttentionRequired),
        other => Err(SchedulerDatabaseError::InvalidValue(format!(
            "unsupported legacy trigger status: {other}"
        ))),
    }
}

fn legacy_occurrence_matches(
    occurrence: &ScheduledOccurrence,
    occurrence_id: &str,
    trigger: &ScheduledTriggerRecord,
    status: OccurrenceStatus,
) -> bool {
    occurrence.id == occurrence_id
        && occurrence.scheduled_at.timestamp_millis() == trigger.scheduled_at.timestamp_millis()
        && occurrence.status == status
        && occurrence.attempt == trigger.attempts
        && occurrence.task_id == trigger.task_id
        && occurrence.run_id == trigger.run_id
}

fn job_exists_tx(transaction: &Transaction<'_>, project_id: &str, job_id: &str) -> Result<bool> {
    transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM scheduled_jobs
                 WHERE project_id = ?1 AND id = ?2 AND definition_json IS NOT NULL
             )",
            params![project_id, job_id],
            |row| row.get(0),
        )
        .map_err(SchedulerDatabaseError::from)
}

fn insert_or_get_occurrence_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    scheduled_at: DateTime<Utc>,
    trigger_kind: OccurrenceTriggerKind,
    now: DateTime<Utc>,
) -> Result<ScheduledOccurrence> {
    transaction.execute(
        "INSERT OR IGNORE INTO scheduled_occurrences (
             id, job_id, scheduled_at, trigger_kind, status, attempt,
             created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?5)",
        params![
            format!("occurrence-{}", Uuid::new_v4()),
            job_id,
            timestamp_millis(scheduled_at),
            trigger_kind.to_string(),
            timestamp_millis(now),
        ],
    )?;
    load_occurrence_by_key(transaction, job_id, scheduled_at, trigger_kind)?.ok_or_else(|| {
        SchedulerDatabaseError::InvalidValue(
            "occurrence was not available after insertion".to_string(),
        )
    })
}

fn mark_missed_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    scheduled_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<bool> {
    if load_occurrence_by_key(
        transaction,
        job_id,
        scheduled_at,
        OccurrenceTriggerKind::Scheduled,
    )?
    .is_none()
    {
        transaction.execute(
            "INSERT INTO scheduled_occurrences (
                 id, job_id, scheduled_at, trigger_kind, status, attempt,
                 created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'scheduled', 'pending', 0, ?4, ?4)",
            params![
                format!("occurrence-{}", Uuid::new_v4()),
                job_id,
                timestamp_millis(scheduled_at),
                timestamp_millis(now),
            ],
        )?;
    }
    let updated = transaction.execute(
        "UPDATE scheduled_occurrences
         SET status = 'missed',
             owner_id = NULL,
             lease_until = NULL,
             heartbeat_at = NULL,
             finished_at = ?3,
             updated_at = ?3
         WHERE job_id = ?1
           AND scheduled_at = ?2
           AND trigger_kind = 'scheduled'
           AND status IN ('pending', 'retrying')",
        params![
            job_id,
            timestamp_millis(scheduled_at),
            timestamp_millis(now)
        ],
    )?;
    Ok(updated == 1)
}

#[cfg(test)]
fn ensure_job_row(transaction: &Transaction<'_>, job_id: &str, now: DateTime<Utc>) -> Result<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO scheduled_jobs (id, created_at, updated_at)
         VALUES (?1, ?2, ?2)",
        params![job_id, timestamp_millis(now)],
    )?;
    Ok(())
}

fn load_job_record(
    connection: &Connection,
    project_id: &str,
    job_id: &str,
) -> Result<Option<ScheduledJobRecord>> {
    connection
        .query_row(
            "SELECT definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE project_id = ?1 AND id = ?2 AND definition_json IS NOT NULL",
            params![project_id, job_id],
            map_job_record,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn load_job_record_tx(
    transaction: &Transaction<'_>,
    project_id: &str,
    job_id: &str,
) -> Result<Option<ScheduledJobRecord>> {
    transaction
        .query_row(
            "SELECT definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE project_id = ?1 AND id = ?2 AND definition_json IS NOT NULL",
            params![project_id, job_id],
            map_job_record,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn map_job_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledJobRecord> {
    let definition_json = row.get::<_, String>(0)?;
    let definition = serde_json::from_str(&definition_json).map_err(to_conversion_error)?;
    let next_run_at = row
        .get::<_, Option<i64>>(2)?
        .map(from_timestamp_millis)
        .transpose()
        .map_err(to_conversion_error)?;
    Ok(ScheduledJobRecord {
        definition,
        revision: row.get(1)?,
        next_run_at,
    })
}

fn map_recoverable_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecoverableScheduledJob> {
    let job = map_job_record(row)?;
    let earliest_running_lease_until = row
        .get::<_, Option<i64>>(4)?
        .map(from_timestamp_millis)
        .transpose()
        .map_err(to_conversion_error)?;
    Ok(RecoverableScheduledJob {
        job,
        has_runnable_occurrence: row.get(3)?,
        earliest_running_lease_until,
    })
}

fn occurrence_claim_state(
    transaction: &Transaction<'_>,
    id: &str,
) -> Result<Option<(String, Option<String>, Option<i64>)>> {
    Ok(transaction
        .query_row(
            "SELECT status, owner_id, lease_until FROM scheduled_occurrences WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

fn load_occurrence_by_id(source: &Connection, id: &str) -> Result<Option<ScheduledOccurrence>> {
    let mut statement = source.prepare(
        "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                error_code, error_params, started_at, finished_at, created_at, updated_at
         FROM scheduled_occurrences WHERE id = ?1",
    )?;
    statement
        .query_row(params![id], map_occurrence)
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn load_occurrence_by_id_tx(
    source: &Transaction<'_>,
    id: &str,
) -> Result<Option<ScheduledOccurrence>> {
    source
        .query_row(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences WHERE id = ?1",
            params![id],
            map_occurrence,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn load_occurrence_by_key(
    transaction: &Transaction<'_>,
    job_id: &str,
    scheduled_at: DateTime<Utc>,
    trigger_kind: OccurrenceTriggerKind,
) -> Result<Option<ScheduledOccurrence>> {
    transaction
        .query_row(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences
             WHERE job_id = ?1 AND scheduled_at = ?2 AND trigger_kind = ?3",
            params![
                job_id,
                timestamp_millis(scheduled_at),
                trigger_kind.to_string()
            ],
            map_occurrence,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn map_occurrence(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledOccurrence> {
    let scheduled_at = from_timestamp_millis(row.get(2)?).map_err(to_conversion_error)?;
    let trigger_kind = row
        .get::<_, String>(3)?
        .parse::<OccurrenceTriggerKind>()
        .map_err(to_conversion_error)?;
    let status = row
        .get::<_, String>(4)?
        .parse::<OccurrenceStatus>()
        .map_err(to_conversion_error)?;
    let error_code = row
        .get::<_, Option<String>>(13)?
        .map(|value| {
            value
                .parse::<ScheduledErrorCode>()
                .map_err(to_conversion_error)
        })
        .transpose()?;
    let error_params = row
        .get::<_, Option<String>>(14)?
        .map(|value| serde_json::from_str(&value).map_err(to_conversion_error))
        .transpose()?;
    Ok(ScheduledOccurrence {
        id: row.get(0)?,
        job_id: row.get(1)?,
        scheduled_at,
        trigger_kind,
        status,
        attempt: row
            .get::<_, i64>(5)?
            .try_into()
            .map_err(to_conversion_error)?,
        owner_id: row.get(6)?,
        lease_until: row
            .get::<_, Option<i64>>(7)?
            .map(from_timestamp_millis)
            .transpose()
            .map_err(to_conversion_error)?,
        heartbeat_at: row
            .get::<_, Option<i64>>(8)?
            .map(from_timestamp_millis)
            .transpose()
            .map_err(to_conversion_error)?,
        task_id: row.get(9)?,
        run_id: row.get(10)?,
        round_id: row.get(11)?,
        attempt_id: row.get(12)?,
        error_code,
        error_params,
        started_at: row
            .get::<_, Option<i64>>(15)?
            .map(from_timestamp_millis)
            .transpose()
            .map_err(to_conversion_error)?,
        finished_at: row
            .get::<_, Option<i64>>(16)?
            .map(from_timestamp_millis)
            .transpose()
            .map_err(to_conversion_error)?,
        created_at: from_timestamp_millis(row.get(17)?).map_err(to_conversion_error)?,
        updated_at: from_timestamp_millis(row.get(18)?).map_err(to_conversion_error)?,
    })
}

fn timestamp_millis(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

fn from_timestamp_millis(value: i64) -> std::result::Result<DateTime<Utc>, String> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or_else(|| format!("invalid UTC timestamp in scheduler database: {value}"))
}

fn to_conversion_error(error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DueMaterialization, LEGACY_JSON_MIGRATION, LEGACY_SHARED_DB_MIGRATION,
        LegacySchedulerSnapshot, ScheduledTaskDatabase, UpdateJobResult,
    };
    use crate::scheduler::occurrence::{
        ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind,
    };
    use crate::scheduler::store::{ScheduledTaskStore, ScheduledTriggerRecord};
    use crate::scheduler::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use camino::Utf8PathBuf;
    use chrono::{Duration, TimeZone, Utc};
    use rusqlite::{Connection, params};
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn database() -> (tempfile::TempDir, ScheduledTaskDatabase) {
        let temp = tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(temp.path().join("scheduled-tasks.db")).unwrap();
        let database = ScheduledTaskDatabase::open(db_path).unwrap();
        (temp, database)
    }

    fn fixed_time() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 5, 10, 0, 0).unwrap()
    }

    fn definition(
        project_id: &str,
        job_id: &str,
        schedule: ScheduleSpec,
    ) -> ScheduledTaskDefinition {
        let now = fixed_time();
        let mut definition = ScheduledTaskDefinition::new(
            project_id,
            job_id,
            "direct",
            schedule,
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.created_at = now;
        definition.updated_at = now;
        definition
    }

    fn migration_exists(database: &ScheduledTaskDatabase, name: &str) -> bool {
        let connection = database.connection.lock().unwrap();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM scheduler_migrations WHERE name = ?1)",
                params![name],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn tolerant_definition_scan_isolates_malformed_rows() {
        let (_temp, database) = database();
        let valid = definition(
            "project-a",
            "job-valid",
            ScheduleSpec::every(1, "hours", fixed_time()).unwrap(),
        );
        let malformed = definition(
            "project-a",
            "job-malformed",
            ScheduleSpec::every(1, "hours", fixed_time()).unwrap(),
        );
        database.save_job_definition(&valid).unwrap();
        database.save_job_definition(&malformed).unwrap();
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE scheduled_jobs SET definition_json = ?1 WHERE id = ?2",
                params!["{invalid", malformed.id],
            )
            .unwrap();

        let scan = database.scan_job_definitions().unwrap();

        assert_eq!(scan.definitions, vec![valid]);
        assert_eq!(scan.invalid_count, 1);
        assert!(database.list_job_definitions().is_err());
    }

    fn set_occurrence_state(
        database: &ScheduledTaskDatabase,
        occurrence_id: &str,
        status: &str,
        finished_at: chrono::DateTime<Utc>,
        run_id: Option<&str>,
    ) {
        let connection = database.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE scheduled_occurrences
                 SET status = ?2, finished_at = ?3, run_id = ?4, updated_at = ?3
                 WHERE id = ?1",
                params![
                    occurrence_id,
                    status,
                    finished_at.timestamp_millis(),
                    run_id
                ],
            )
            .unwrap();
    }

    #[test]
    fn schema_v1_migrates_to_v2_without_losing_jobs_or_occurrences() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("scheduled-tasks.db");
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE scheduler_schema (
                     id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL
                 );
                 INSERT INTO scheduler_schema(id, version) VALUES (1, 1);
                 CREATE TABLE scheduled_jobs (
                     id TEXT PRIMARY KEY, project_id TEXT, enabled INTEGER NOT NULL DEFAULT 1,
                     definition_json TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE scheduled_occurrences (
                     id TEXT PRIMARY KEY, job_id TEXT NOT NULL, scheduled_at INTEGER NOT NULL,
                     trigger_kind TEXT NOT NULL, status TEXT NOT NULL, attempt INTEGER NOT NULL DEFAULT 0,
                     owner_id TEXT, lease_until INTEGER, heartbeat_at INTEGER, task_id TEXT, run_id TEXT,
                     round_id TEXT, attempt_id TEXT, error_code TEXT, error_params TEXT,
                     started_at INTEGER, finished_at INTEGER, created_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL, UNIQUE(job_id, scheduled_at, trigger_kind)
                 );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO scheduled_jobs
                 (id, project_id, enabled, definition_json, created_at, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?4)",
                params![
                    definition.id(),
                    definition.project_id,
                    serde_json::to_string(&definition).unwrap(),
                    fixed_time().timestamp_millis()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO scheduled_occurrences
                 (id, job_id, scheduled_at, trigger_kind, status, created_at, updated_at)
                 VALUES ('occurrence-a', ?1, ?2, 'scheduled', 'pending', ?2, ?2)",
                params![definition.id(), fixed_time().timestamp_millis()],
            )
            .unwrap();
        drop(connection);

        let database = ScheduledTaskDatabase::open(&path).unwrap();

        assert_eq!(database.schema_version().unwrap(), 2);
        let job = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert_eq!(job.revision, 1);
        assert_eq!(job.next_run_at, Some(fixed_time() + Duration::hours(1)));
        assert_eq!(job.definition, definition);
        assert_eq!(database.list_occurrences("job-a", 10).unwrap().len(), 1);
    }

    #[test]
    fn newer_schema_version_is_rejected_without_downgrade() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("scheduled-tasks.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE scheduler_schema (
                     id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL
                 );
                 INSERT INTO scheduler_schema(id, version) VALUES (1, 3);",
            )
            .unwrap();
        drop(connection);

        assert!(matches!(
            ScheduledTaskDatabase::open(&path),
            Err(super::SchedulerDatabaseError::UnsupportedSchemaVersion {
                found: 3,
                supported: 2
            })
        ));
        let connection = Connection::open(&path).unwrap();
        let version = connection
            .query_row("SELECT version FROM scheduler_schema", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn legacy_json_import_writes_marker_only_after_full_transaction_commits() {
        let (_temp, database) = database();
        let existing = definition(
            "project-a",
            "job-conflict",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        database.create_job(&existing, None).unwrap();
        let new_job = definition(
            "project-a",
            "job-new",
            ScheduleSpec::at(fixed_time() + Duration::hours(2)),
        );
        let mut conflicting = existing.clone();
        conflicting.instruction = "different".to_string();
        let snapshot = LegacySchedulerSnapshot {
            definitions: vec![new_job, conflicting],
            triggers: BTreeMap::new(),
        };

        assert!(
            database
                .import_legacy_snapshot_once(&snapshot, fixed_time())
                .is_err()
        );
        assert!(
            database
                .get_job_definition("project-a", "job-new")
                .unwrap()
                .is_none()
        );
        assert!(!migration_exists(&database, LEGACY_JSON_MIGRATION));
    }

    #[test]
    fn an_empty_database_with_completed_marker_does_not_reimport_json() {
        let (_temp, database) = database();
        let empty = LegacySchedulerSnapshot {
            definitions: Vec::new(),
            triggers: BTreeMap::new(),
        };
        assert_eq!(
            database
                .import_legacy_snapshot_once(&empty, fixed_time())
                .unwrap(),
            0
        );
        assert!(migration_exists(&database, LEGACY_JSON_MIGRATION));

        let populated = LegacySchedulerSnapshot {
            definitions: vec![definition(
                "project-a",
                "job-late",
                ScheduleSpec::at(fixed_time() + Duration::hours(1)),
            )],
            triggers: BTreeMap::new(),
        };
        assert_eq!(
            database
                .import_legacy_snapshot_once(&populated, fixed_time())
                .unwrap(),
            0
        );
        assert!(
            database
                .get_job_definition("project-a", "job-late")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn completed_json_marker_skips_reading_corrupt_legacy_store() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = crate::storage::GoldBandPaths::new(repo_root);
        let database = ScheduledTaskDatabase::open(paths.scheduler_db_path()).unwrap();
        let empty = LegacySchedulerSnapshot {
            definitions: Vec::new(),
            triggers: BTreeMap::new(),
        };
        database
            .import_legacy_snapshot_once(&empty, fixed_time())
            .unwrap();
        let corrupt_path = paths.scheduled_task_file("corrupt");
        std::fs::create_dir_all(corrupt_path.parent().unwrap()).unwrap();
        std::fs::write(corrupt_path, b"not json").unwrap();

        assert_eq!(
            database
                .import_legacy_store(&ScheduledTaskStore::new(paths))
                .unwrap(),
            0
        );
    }

    #[test]
    fn legacy_shared_database_is_copied_once_per_project_and_marked() {
        let temp = tempdir().unwrap();
        let source = ScheduledTaskDatabase::open(temp.path().join("shared.db")).unwrap();
        let destination = ScheduledTaskDatabase::open(temp.path().join("project-a.db")).unwrap();
        let project_a = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        let project_b = definition(
            "project-b",
            "job-b",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        source.create_job(&project_a, Some(fixed_time())).unwrap();
        source.create_job(&project_b, Some(fixed_time())).unwrap();

        assert_eq!(
            destination
                .import_legacy_database_once(&source, "project-a", fixed_time())
                .unwrap(),
            1
        );
        assert_eq!(
            destination
                .import_legacy_database_once(&source, "project-a", fixed_time())
                .unwrap(),
            0
        );
        assert!(migration_exists(&destination, LEGACY_SHARED_DB_MIGRATION));
        assert!(
            destination
                .get_job_definition("project-a", "job-a")
                .unwrap()
                .is_some()
        );
        assert!(
            destination
                .get_job_definition("project-b", "job-b")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn completed_shared_database_marker_skips_opening_corrupt_legacy_source() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("shared.db");
        let destination = ScheduledTaskDatabase::open(temp.path().join("project-a.db")).unwrap();
        let source = ScheduledTaskDatabase::open(&source_path).unwrap();

        assert_eq!(
            destination
                .import_legacy_database_path_once(&source_path, "project-a", fixed_time())
                .unwrap(),
            0
        );
        drop(source);
        std::fs::write(&source_path, b"not a sqlite database").unwrap();

        assert_eq!(
            destination
                .import_legacy_database_path_once(&source_path, "project-a", fixed_time())
                .unwrap(),
            0
        );
    }

    #[test]
    fn legacy_shared_database_derives_missing_enabled_deadline() {
        let temp = tempdir().unwrap();
        let source = ScheduledTaskDatabase::open(temp.path().join("shared.db")).unwrap();
        let destination = ScheduledTaskDatabase::open(temp.path().join("project-a.db")).unwrap();
        let deadline = fixed_time() + Duration::hours(1);
        let definition = definition("project-a", "job-a", ScheduleSpec::at(deadline));
        source.create_job(&definition, None).unwrap();

        destination
            .import_legacy_database_once(&source, "project-a", fixed_time())
            .unwrap();

        let imported = destination
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert_eq!(imported.next_run_at, Some(deadline));
    }

    #[test]
    fn get_job_definition_is_scoped_by_project_and_job_id() {
        let (_temp, database) = database();
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        database
            .create_job(&definition, Some(fixed_time()))
            .unwrap();

        assert!(
            database
                .get_job_definition("project-a", "job-a")
                .unwrap()
                .is_some()
        );
        assert!(
            database
                .get_job_definition("project-b", "job-a")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn optimistic_update_rejects_stale_updated_at() {
        let (_temp, database) = database();
        let original = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        database.create_job(&original, Some(fixed_time())).unwrap();
        let mut first_update = original.clone();
        first_update.instruction = "first".to_string();
        first_update.updated_at = fixed_time() + Duration::seconds(1);
        assert!(matches!(
            database
                .update_job(&first_update, original.updated_at, Some(fixed_time()))
                .unwrap(),
            UpdateJobResult::Updated(_)
        ));

        let mut stale_update = original.clone();
        stale_update.instruction = "stale".to_string();
        stale_update.updated_at = fixed_time() + Duration::seconds(2);
        let result = database
            .update_job(&stale_update, original.updated_at, Some(fixed_time()))
            .unwrap();
        assert!(matches!(
            result,
            UpdateJobResult::Conflict(current)
                if current.definition.instruction == "first" && current.revision == 2
        ));
    }

    #[test]
    fn optimistic_update_requires_a_newer_persisted_timestamp() {
        let (_temp, database) = database();
        let original = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        database.create_job(&original, Some(fixed_time())).unwrap();
        let mut update = original.clone();
        update.instruction = "changed without advancing the token".to_string();

        assert!(matches!(
            database.update_job(&update, original.updated_at, Some(fixed_time())),
            Err(super::SchedulerDatabaseError::InvalidValue(_))
        ));
        let stored = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert!(stored.definition.instruction.is_empty());
        assert_eq!(stored.revision, 1);
    }

    #[test]
    fn manual_occurrence_does_not_change_next_run_at() {
        let (_temp, database) = database();
        let next_run_at = fixed_time() + Duration::hours(1);
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::every(1, "hours", next_run_at).unwrap(),
        );
        database.create_job(&definition, Some(next_run_at)).unwrap();
        let before = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();

        database
            .create_or_get_occurrence(definition.id(), fixed_time(), OccurrenceTriggerKind::Manual)
            .unwrap();

        let after = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert_eq!(after.next_run_at, before.next_run_at);
        assert_eq!(after.revision, before.revision);
    }

    #[test]
    fn due_materialization_atomically_creates_occurrence_and_advances_next_run_at() {
        let (_temp, database) = database();
        let now = fixed_time();
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::every(1, "hours", now).unwrap(),
        );
        let created = database.create_job(&definition, Some(now)).unwrap();

        let materialized = database
            .materialize_due_occurrence("project-a", "job-a", created.revision, now)
            .unwrap();

        let DueMaterialization::Ready { job, occurrence } = materialized else {
            panic!("expected a due occurrence");
        };
        assert_eq!(occurrence.scheduled_at, now);
        assert_eq!(occurrence.trigger_kind, OccurrenceTriggerKind::Scheduled);
        assert_eq!(job.next_run_at, Some(now + Duration::hours(1)));
        assert_eq!(job.revision, created.revision + 1);
        let stored = database
            .get_job_definition("project-a", "job-a")
            .unwrap()
            .unwrap();
        assert_eq!(stored, job);
    }

    #[test]
    fn stale_revision_cannot_materialize_a_due_occurrence() {
        let (_temp, database) = database();
        let now = fixed_time();
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::every(1, "hours", now).unwrap(),
        );
        let created = database.create_job(&definition, Some(now)).unwrap();
        assert!(matches!(
            database
                .materialize_due_occurrence("project-a", "job-a", created.revision + 1, now)
                .unwrap(),
            DueMaterialization::Stale
        ));
        assert!(database.list_occurrences("job-a", 10).unwrap().is_empty());
    }

    #[test]
    fn runtime_projection_update_preserves_persisted_deadline() {
        let (_temp, database) = database();
        let deadline = fixed_time() + Duration::hours(1);
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::every(1, "hours", deadline).unwrap(),
        );
        let created = database.create_job(&definition, Some(deadline)).unwrap();
        let mut projection = definition.clone();
        projection.task_id = Some("task-a".to_string());
        projection.last_trigger_status = Some("succeeded".to_string());

        let result = database
            .update_job_runtime_projection(&projection, created.revision)
            .unwrap();

        let UpdateJobResult::Updated(projected) = result else {
            panic!("expected runtime projection update");
        };
        assert_eq!(projected.next_run_at, Some(deadline));
        assert_eq!(projected.definition.task_id.as_deref(), Some("task-a"));
        assert!(projected.definition.updated_at > definition.updated_at);
        let stored_updated_at = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT updated_at FROM scheduled_jobs WHERE id = 'job-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(
            stored_updated_at,
            projected.definition.updated_at.timestamp_millis()
        );

        let mut stale_authoring = definition.clone();
        stale_authoring.instruction = "stale authoring update".to_string();
        stale_authoring.updated_at = fixed_time() + Duration::seconds(2);
        let result = database
            .update_job(&stale_authoring, definition.updated_at, Some(deadline))
            .unwrap();
        assert!(matches!(
            result,
            UpdateJobResult::Conflict(current)
                if current.definition.task_id.as_deref() == Some("task-a")
                    && current.definition.instruction.is_empty()
        ));
    }

    #[test]
    fn enabled_job_lifecycle_is_project_scoped() {
        let (_temp, database) = database();
        let enabled = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        let mut disabled = definition(
            "project-a",
            "job-b",
            ScheduleSpec::at(fixed_time() + Duration::hours(2)),
        );
        disabled.enabled = false;
        database.create_job(&enabled, Some(fixed_time())).unwrap();
        database.create_job(&disabled, None).unwrap();

        assert_eq!(database.enabled_job_count().unwrap(), 1);
        assert_eq!(database.list_enabled_jobs().unwrap().len(), 1);
        assert!(matches!(
            database
                .set_job_enabled(
                    "project-a",
                    "job-a",
                    enabled.updated_at,
                    false,
                    None,
                )
                .unwrap(),
            UpdateJobResult::Updated(record) if !record.definition.enabled
        ));
        assert_eq!(database.enabled_job_count().unwrap(), 0);
        assert!(!database.delete_job("project-b", "job-a").unwrap());
        assert!(database.delete_job("project-a", "job-a").unwrap());
        assert!(
            database
                .get_job_definition("project-a", "job-b")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn attention_occurrence_can_be_resumed_by_one_owner() {
        let (_temp, database) = database();
        let now = fixed_time();
        let occurrence = database
            .create_or_get_occurrence("job-a", now, OccurrenceTriggerKind::Manual)
            .unwrap();
        set_occurrence_state(&database, &occurrence.id, "attention_required", now, None);

        let resumed = database
            .resume_attention_occurrence(&occurrence.id, "owner-a", now, now + Duration::minutes(5))
            .unwrap();

        assert!(matches!(
            resumed,
            ClaimResult::Claimed(current)
                if current.status == OccurrenceStatus::Running
                    && current.owner_id.as_deref() == Some("owner-a")
        ));
        assert!(matches!(
            database
                .resume_attention_occurrence(
                    &occurrence.id,
                    "owner-b",
                    now,
                    now + Duration::minutes(5),
                )
                .unwrap(),
            ClaimResult::Busy
        ));
    }

    #[test]
    fn attention_occurrence_can_be_found_by_execution_links() {
        let (_temp, database) = database();
        let now = fixed_time();
        let definition = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(now + Duration::hours(1)),
        );
        database.create_job(&definition, Some(now)).unwrap();
        let occurrence = database
            .create_or_get_occurrence("job-a", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, now + Duration::minutes(5))
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        database
            .accept_occurrence_links(
                &occurrence.id,
                "owner-a",
                now,
                &OccurrenceLinks {
                    task_id: Some("task-a".to_string()),
                    run_id: Some("run-a".to_string()),
                    round_id: Some("round-a".to_string()),
                    attempt_id: Some("attempt-a".to_string()),
                },
            )
            .unwrap();
        set_occurrence_state(
            &database,
            &occurrence.id,
            "attention_required",
            now,
            Some("run-a"),
        );
        let found = database
            .find_attention_occurrence_by_links("task-a", "run-a", "round-a", "attempt-a")
            .unwrap()
            .expect("attention occurrence should be found");
        assert_eq!(found.id, occurrence.id);
        assert_eq!(found.status, OccurrenceStatus::AttentionRequired);
    }

    #[test]
    fn retention_deletes_only_old_terminal_unlinked_occurrences() {
        let (_temp, database) = database();
        let now = fixed_time();
        let old = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(40),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        let older = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(41),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        let recent = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(1),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        let nonterminal = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(39),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();
        set_occurrence_state(
            &database,
            &old.id,
            "succeeded",
            now - Duration::days(40),
            None,
        );
        set_occurrence_state(
            &database,
            &older.id,
            "skipped",
            now - Duration::days(41),
            None,
        );
        set_occurrence_state(
            &database,
            &recent.id,
            "failed",
            now - Duration::days(1),
            None,
        );

        let result = database
            .cleanup_terminal_occurrences(now - Duration::days(30), 1, &HashSet::new())
            .unwrap();

        assert_eq!(result.deleted, 1);
        assert!(result.has_more);
        let result = database
            .cleanup_terminal_occurrences(now - Duration::days(30), 1, &HashSet::new())
            .unwrap();
        assert_eq!(result.deleted, 1);
        assert!(!result.has_more);
        assert!(database.get_occurrence(&old.id).unwrap().is_none());
        assert!(database.get_occurrence(&older.id).unwrap().is_none());
        assert!(database.get_occurrence(&recent.id).unwrap().is_some());
        assert!(database.get_occurrence(&nonterminal.id).unwrap().is_some());
    }

    #[test]
    fn retention_preserves_attention_and_nonterminal_run_links() {
        let (_temp, database) = database();
        let now = fixed_time();
        let attention = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(40),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        let protected = database
            .create_or_get_occurrence(
                "job-a",
                now - Duration::days(41),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        set_occurrence_state(
            &database,
            &attention.id,
            "attention_required",
            now - Duration::days(40),
            None,
        );
        set_occurrence_state(
            &database,
            &protected.id,
            "succeeded",
            now - Duration::days(41),
            Some("run-active"),
        );

        let result = database
            .cleanup_terminal_occurrences(
                now - Duration::days(30),
                500,
                &HashSet::from(["run-active".to_string()]),
            )
            .unwrap();

        assert_eq!(result.deleted, 0);
        assert!(database.get_occurrence(&attention.id).unwrap().is_some());
        assert!(database.get_occurrence(&protected.id).unwrap().is_some());
    }

    #[test]
    fn retention_rejects_zero_batch_size() {
        let (_temp, database) = database();

        assert!(matches!(
            database.cleanup_terminal_occurrences(fixed_time(), 0, &HashSet::new()),
            Err(super::SchedulerDatabaseError::InvalidValue(_))
        ));
    }

    #[test]
    fn derived_deadline_keeps_disabled_and_completed_one_shot_unscheduled() {
        let deadline = fixed_time() + Duration::hours(1);
        let mut disabled = definition("project-a", "job-disabled", ScheduleSpec::at(deadline));
        disabled.enabled = false;
        let mut completed = definition("project-a", "job-completed", ScheduleSpec::at(deadline));
        completed.last_trigger_at = Some(deadline);

        assert_eq!(super::derived_next_run_at(&disabled), None);
        assert_eq!(super::derived_next_run_at(&completed), None);
    }

    #[test]
    fn scheduled_occurrence_is_unique_by_job_time_and_trigger_kind() {
        let (_temp, database) = database();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();

        let first = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let second = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();

        assert_eq!(first.id, second.id);
        let manual = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Manual)
            .unwrap();
        assert_ne!(first.id, manual.id);
        assert_eq!(database.list_occurrences("job-1", 10).unwrap().len(), 2);
    }

    #[test]
    fn open_creates_missing_database_parent_directory() {
        let temp = tempdir().unwrap();
        let db_path =
            Utf8PathBuf::from_path_buf(temp.path().join("nested").join("scheduled-tasks.db"))
                .unwrap();

        ScheduledTaskDatabase::open(&db_path).unwrap();

        assert!(db_path.exists());
    }

    #[test]
    fn only_one_owner_can_claim_an_occurrence() {
        let (_temp, database) = database();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let occurrence = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let now = scheduled_at - Duration::minutes(1);
        let lease_until = now + Duration::minutes(1);

        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, lease_until)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-b", now, lease_until)
                .unwrap(),
            ClaimResult::AlreadyOwned | ClaimResult::Busy
        ));
        assert_eq!(
            database
                .list_occurrences("job-1", 10)
                .unwrap()
                .iter()
                .filter(|item| item.status == OccurrenceStatus::Running)
                .count(),
            1
        );
    }

    #[test]
    fn cross_connection_claim_is_atomic() {
        let temp = tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(temp.path().join("scheduled-tasks.db")).unwrap();
        let first_database = ScheduledTaskDatabase::open(&db_path).unwrap();
        let second_database = ScheduledTaskDatabase::open(&db_path).unwrap();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let occurrence = first_database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let first_barrier = Arc::clone(&barrier);
        let second_barrier = Arc::clone(&barrier);
        let first_id = occurrence.id.clone();
        let second_id = occurrence.id.clone();
        let first_thread = thread::spawn(move || {
            first_barrier.wait();
            first_database
                .claim_occurrence(
                    &first_id,
                    "owner-a",
                    scheduled_at,
                    scheduled_at + Duration::minutes(5),
                )
                .unwrap()
        });
        let second_thread = thread::spawn(move || {
            second_barrier.wait();
            second_database
                .claim_occurrence(
                    &second_id,
                    "owner-b",
                    scheduled_at,
                    scheduled_at + Duration::minutes(5),
                )
                .unwrap()
        });
        barrier.wait();

        let first_result = first_thread.join().unwrap();
        let second_result = second_thread.join().unwrap();
        assert_ne!(first_result.is_claimed(), second_result.is_claimed());
        assert!(first_result.is_claimed() || second_result.is_claimed());
    }

    #[test]
    fn expired_lease_can_be_reclaimed() {
        let (_temp, database) = database();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let occurrence = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let first_now = scheduled_at - Duration::minutes(2);
        let first_lease = scheduled_at - Duration::minutes(1);
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", first_now, first_lease)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));

        let reclaimed_at = scheduled_at;
        let reclaimed_until = reclaimed_at + Duration::minutes(1);
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-b", reclaimed_at, reclaimed_until)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));

        let current = database.list_occurrences("job-1", 10).unwrap();
        assert_eq!(current[0].owner_id.as_deref(), Some("owner-b"));
        assert_eq!(current[0].attempt, 2);
    }

    #[test]
    fn renew_lease_requires_current_owner_and_updates_heartbeat() {
        let (_temp, database) = database();
        let scheduled_at = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", scheduled_at, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let lease_until = scheduled_at + Duration::minutes(5);
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", scheduled_at, lease_until)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));

        let heartbeat_at = scheduled_at + Duration::seconds(1);
        assert!(
            database
                .renew_lease(
                    &occurrence.id,
                    "owner-a",
                    heartbeat_at,
                    heartbeat_at + Duration::minutes(5)
                )
                .unwrap()
        );
        assert!(
            !database
                .renew_lease(
                    &occurrence.id,
                    "owner-b",
                    heartbeat_at,
                    heartbeat_at + Duration::minutes(5)
                )
                .unwrap()
        );

        let current = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(
            current.heartbeat_at,
            Some(
                Utc.timestamp_millis_opt(heartbeat_at.timestamp_millis())
                    .unwrap()
            )
        );
    }

    #[test]
    fn accept_occurrence_links_requires_current_owner_and_live_lease() {
        let (_temp, database) = database();
        let now = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let lease_until = now + Duration::minutes(5);
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, lease_until)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let links = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            round_id: Some("round-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };

        assert!(
            !database
                .accept_occurrence_links(&occurrence.id, "owner-b", now, &links)
                .unwrap()
        );
        assert!(
            !database
                .accept_occurrence_links(&occurrence.id, "owner-a", lease_until, &links)
                .unwrap()
        );
        assert!(
            database
                .accept_occurrence_links(
                    &occurrence.id,
                    "owner-a",
                    now + Duration::seconds(1),
                    &links,
                )
                .unwrap()
        );

        let accepted = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(accepted.status, OccurrenceStatus::Running);
        assert_eq!(accepted.owner_id.as_deref(), Some("owner-a"));
        assert_eq!(
            accepted.lease_until,
            Some(
                Utc.timestamp_millis_opt(lease_until.timestamp_millis())
                    .unwrap()
            )
        );
        assert_eq!(accepted.links(), links);
    }

    #[test]
    fn finish_without_links_preserves_accepted_occurrence_links() {
        let (_temp, database) = database();
        let now = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        let lease_until = now + Duration::minutes(5);
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, lease_until)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let links = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            round_id: Some("round-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
        };
        assert!(
            database
                .accept_occurrence_links(&occurrence.id, "owner-a", now, &links)
                .unwrap()
        );

        assert!(
            database
                .finish_occurrence(
                    &occurrence.id,
                    "owner-a",
                    OccurrenceStatus::Failed,
                    None,
                    Some(super::ScheduledError::new(
                        super::ScheduledErrorCode::ExecutionFailed,
                    )),
                )
                .unwrap()
        );

        let finished = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(finished.status, OccurrenceStatus::Failed);
        assert_eq!(finished.links(), links);
    }

    #[test]
    fn accepted_occurrence_links_can_only_fill_missing_fields() {
        let (_temp, database) = database();
        let now = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, now + Duration::minutes(5),)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let task_only = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            ..Default::default()
        };
        assert!(
            database
                .accept_occurrence_links(&occurrence.id, "owner-a", now, &task_only)
                .unwrap()
        );
        let with_run = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            ..Default::default()
        };
        assert!(
            database
                .accept_occurrence_links(&occurrence.id, "owner-a", now, &with_run)
                .unwrap()
        );
        let conflicting = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-2".to_string()),
            ..Default::default()
        };

        assert!(
            !database
                .accept_occurrence_links(&occurrence.id, "owner-a", now, &conflicting)
                .unwrap()
        );
        assert_eq!(
            database
                .get_occurrence(&occurrence.id)
                .unwrap()
                .unwrap()
                .links(),
            with_run
        );
    }

    #[test]
    fn finish_occurrence_requires_owner_and_persists_links_and_error() {
        let (_temp, database) = database();
        let now = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Manual)
            .unwrap();
        assert!(matches!(
            database
                .claim_occurrence(&occurrence.id, "owner-a", now, now + Duration::minutes(5))
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        let links = super::OccurrenceLinks {
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            ..Default::default()
        };
        let error = super::ScheduledError::with_params(
            super::ScheduledErrorCode::ExecutionFailed,
            serde_json::json!({"reason": "test"}),
        );

        assert!(
            !database
                .finish_occurrence(
                    &occurrence.id,
                    "owner-b",
                    OccurrenceStatus::Failed,
                    Some(links.clone()),
                    Some(error.clone()),
                )
                .unwrap()
        );
        assert!(
            database
                .finish_occurrence(
                    &occurrence.id,
                    "owner-a",
                    OccurrenceStatus::Failed,
                    Some(links),
                    Some(error),
                )
                .unwrap()
        );

        let current = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(current.status, OccurrenceStatus::Failed);
        assert_eq!(current.task_id.as_deref(), Some("task-1"));
        assert_eq!(current.run_id.as_deref(), Some("run-1"));
        assert_eq!(
            current.error_code,
            Some(super::ScheduledErrorCode::ExecutionFailed)
        );
        assert_eq!(current.owner_id, None);
        assert!(current.finished_at.is_some());
    }

    #[test]
    fn recover_expired_releases_running_occurrence_for_retry() {
        let (_temp, database) = database();
        let now = Utc::now();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        assert!(matches!(
            database
                .claim_occurrence(
                    &occurrence.id,
                    "owner-a",
                    now - Duration::minutes(2),
                    now - Duration::minutes(1),
                )
                .unwrap(),
            ClaimResult::Claimed(_)
        ));

        assert_eq!(database.recover_expired(now).unwrap(), 1);
        let current = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(current.status, OccurrenceStatus::Retrying);
        assert_eq!(current.owner_id, None);
        assert_eq!(
            current.error_code,
            Some(super::ScheduledErrorCode::LeaseLost)
        );
    }

    #[test]
    fn project_recovery_view_includes_disabled_nonterminal_jobs_and_earliest_lease() {
        let (_temp, database) = database();
        let now = fixed_time();
        let deadline = now + Duration::hours(2);

        let enabled = definition("project-a", "enabled-job", ScheduleSpec::at(deadline));
        database.create_job(&enabled, Some(deadline)).unwrap();

        let mut pending = definition("project-a", "disabled-pending", ScheduleSpec::at(deadline));
        pending.enabled = false;
        database.create_job(&pending, None).unwrap();
        database
            .create_or_get_occurrence(
                pending.id(),
                now - Duration::minutes(1),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();

        let mut running = definition("project-a", "disabled-running", ScheduleSpec::at(deadline));
        running.enabled = false;
        database.create_job(&running, None).unwrap();
        let first_running = database
            .create_or_get_occurrence(
                running.id(),
                now - Duration::minutes(2),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();
        let second_running = database
            .create_or_get_occurrence(
                running.id(),
                now - Duration::minutes(1),
                OccurrenceTriggerKind::Manual,
            )
            .unwrap();
        let first_lease = now + Duration::minutes(2);
        let second_lease = now + Duration::minutes(1);
        database
            .claim_occurrence(&first_running.id, "owner-a", now, first_lease)
            .unwrap();
        database
            .claim_occurrence(&second_running.id, "owner-b", now, second_lease)
            .unwrap();

        let mut foreign = definition("project-b", "foreign-job", ScheduleSpec::at(deadline));
        foreign.enabled = false;
        database.create_job(&foreign, None).unwrap();
        database
            .create_or_get_occurrence(
                foreign.id(),
                now - Duration::minutes(1),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();

        let recovery = database
            .list_recoverable_jobs_for_project("project-a")
            .unwrap();
        let by_id = recovery
            .into_iter()
            .map(|record| (record.job.definition.id().to_string(), record))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(by_id.len(), 3);
        assert!(by_id["enabled-job"].job.definition.enabled);
        assert!(!by_id["enabled-job"].has_runnable_occurrence);
        assert!(by_id["disabled-pending"].has_runnable_occurrence);
        assert_eq!(
            by_id["disabled-running"].earliest_running_lease_until,
            Some(second_lease)
        );
        assert!(!by_id.contains_key("foreign-job"));
    }

    #[test]
    fn graceful_owner_release_marks_running_occurrence_retrying_with_lease_lost() {
        let (_temp, database) = database();
        let now = fixed_time();
        let occurrence = database
            .create_or_get_occurrence("job-1", now, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        database
            .claim_occurrence(
                &occurrence.id,
                "desktop-owner",
                now,
                now + Duration::minutes(5),
            )
            .unwrap();

        assert!(
            database
                .release_owned_occurrence_for_retry(&occurrence.id, "desktop-owner", now)
                .unwrap()
        );

        let current = database.get_occurrence(&occurrence.id).unwrap().unwrap();
        assert_eq!(current.status, OccurrenceStatus::Retrying);
        assert_eq!(current.owner_id, None);
        assert_eq!(current.lease_until, None);
        assert_eq!(
            current.error_code,
            Some(super::ScheduledErrorCode::LeaseLost)
        );
    }

    #[test]
    fn oldest_runnable_occurrence_is_not_hidden_by_more_than_ten_thousand_terminals() {
        let (_temp, database) = database();
        let oldest = fixed_time();
        database
            .create_or_get_occurrence("job-1", oldest, OccurrenceTriggerKind::Scheduled)
            .unwrap();
        {
            let mut connection = database.connection.lock().unwrap();
            let transaction = connection.transaction().unwrap();
            for offset in 1..=10_001_i64 {
                let scheduled_at = oldest + Duration::seconds(offset);
                transaction
                    .execute(
                        "INSERT INTO scheduled_occurrences (
                             id, job_id, scheduled_at, trigger_kind, status, attempt,
                             finished_at, created_at, updated_at
                         ) VALUES (?1, 'job-1', ?2, 'manual', 'succeeded', 1, ?2, ?2, ?2)",
                        params![
                            format!("terminal-{offset}"),
                            scheduled_at.timestamp_millis()
                        ],
                    )
                    .unwrap();
            }
            transaction.commit().unwrap();
        }

        let occurrence = database
            .oldest_runnable_occurrence("job-1")
            .unwrap()
            .unwrap();

        assert_eq!(occurrence.scheduled_at, oldest);
        assert_eq!(occurrence.status, OccurrenceStatus::Pending);
    }

    #[test]
    fn mark_missed_creates_and_finishes_a_scheduled_occurrence() {
        let (_temp, database) = database();
        let scheduled_at = Utc::now() - Duration::minutes(1);

        assert!(database.mark_missed("job-1", scheduled_at).unwrap());
        let current = database.list_occurrences("job-1", 10).unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].trigger_kind, OccurrenceTriggerKind::Scheduled);
        assert_eq!(current[0].status, OccurrenceStatus::Missed);
        assert!(current[0].finished_at.is_some());
    }

    #[test]
    fn deleted_due_job_cannot_be_recreated_by_mark_missed() {
        let (_temp, database) = database();
        let now = fixed_time();
        let definition = definition("project-a", "deleted-due-job", ScheduleSpec::at(now));
        let created = database.create_job(&definition, Some(now)).unwrap();
        assert!(matches!(
            database
                .materialize_due_occurrence(
                    &definition.project_id,
                    definition.id(),
                    created.revision,
                    now,
                )
                .unwrap(),
            DueMaterialization::Ready { .. }
        ));
        assert!(
            database
                .delete_job(&definition.project_id, definition.id())
                .unwrap()
        );

        assert_eq!(
            database
                .mark_missed_for_existing_job(&definition.project_id, definition.id(), now)
                .unwrap(),
            None
        );
        assert!(
            database
                .get_job_definition(&definition.project_id, definition.id())
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .list_occurrences(definition.id(), 10)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn legacy_json_definition_import_is_idempotent() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = crate::storage::GoldBandPaths::new(repo_root);
        let store = ScheduledTaskStore::new(paths);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-legacy",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        store.save(&definition).unwrap();
        store
            .append_trigger(ScheduledTriggerRecord::new(
                definition.id(),
                definition.created_at,
                "completed",
                Some("task-1".to_string()),
                Some("run-1".to_string()),
                1,
            ))
            .unwrap();

        let database = ScheduledTaskDatabase::open(temp.path().join("scheduled-tasks.db")).unwrap();
        assert_eq!(database.import_legacy_store(&store).unwrap(), 1);
        assert_eq!(database.import_legacy_store(&store).unwrap(), 0);
        let history = database.list_occurrences(definition.id(), 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].task_id.as_deref(), Some("task-1"));
        assert_eq!(history[0].run_id.as_deref(), Some("run-1"));
        assert_eq!(history[0].status, OccurrenceStatus::Succeeded);
    }

    #[test]
    fn legacy_json_import_replaces_placeholder_and_sets_deadline() {
        let (_temp, database) = database();
        let deadline = fixed_time() + Duration::hours(1);
        database
            .create_or_get_occurrence(
                "job-placeholder",
                fixed_time(),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();
        let snapshot = LegacySchedulerSnapshot {
            definitions: vec![definition(
                "project-a",
                "job-placeholder",
                ScheduleSpec::at(deadline),
            )],
            triggers: BTreeMap::new(),
        };

        assert_eq!(
            database
                .import_legacy_snapshot_once(&snapshot, fixed_time())
                .unwrap(),
            1
        );
        let imported = database
            .get_job_definition("project-a", "job-placeholder")
            .unwrap()
            .unwrap();
        assert_eq!(imported.next_run_at, Some(deadline));
    }

    #[test]
    fn legacy_json_import_backfills_matching_definition_with_null_deadline() {
        let (_temp, database) = database();
        let deadline = fixed_time() + Duration::hours(1);
        let definition = definition("project-a", "job-existing", ScheduleSpec::at(deadline));
        database.create_job(&definition, None).unwrap();
        let snapshot = LegacySchedulerSnapshot {
            definitions: vec![definition],
            triggers: BTreeMap::new(),
        };

        assert_eq!(
            database
                .import_legacy_snapshot_once(&snapshot, fixed_time())
                .unwrap(),
            0
        );
        let imported = database
            .get_job_definition("project-a", "job-existing")
            .unwrap()
            .unwrap();
        assert_eq!(imported.next_run_at, Some(deadline));
    }

    #[test]
    fn legacy_import_rejects_conflicting_timestamps() {
        let temp = tempdir().unwrap();
        let database = ScheduledTaskDatabase::open(temp.path().join("scheduled-tasks.db")).unwrap();
        let scheduled_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-timestamp-conflict",
            "direct",
            ScheduleSpec::at(scheduled_at),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let mut first =
            ScheduledTriggerRecord::new(definition.id(), scheduled_at, "completed", None, None, 1);
        first.id = "trigger-001".to_string();
        let mut second =
            ScheduledTriggerRecord::new(definition.id(), scheduled_at, "completed", None, None, 1);
        second.id = "trigger-002".to_string();
        let snapshot = LegacySchedulerSnapshot {
            definitions: vec![definition.clone()],
            triggers: BTreeMap::from([(definition.id().to_string(), vec![first, second])]),
        };
        let result = database.import_legacy_snapshot_once(&snapshot, fixed_time());
        assert!(matches!(
            result,
            Err(super::SchedulerDatabaseError::MigrationConflict { field, .. })
                if field == "scheduled_at"
        ));
    }

    #[test]
    fn shared_database_migration_copies_only_the_requested_project() {
        let temp = tempdir().unwrap();
        let source = ScheduledTaskDatabase::open(temp.path().join("shared.db")).unwrap();
        let destination = ScheduledTaskDatabase::open(temp.path().join("project-a.db")).unwrap();
        let schedule = ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap());
        let project_a = ScheduledTaskDefinition::new(
            "project-a",
            "job-a",
            "direct",
            schedule.clone(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let project_b = ScheduledTaskDefinition::new(
            "project-b",
            "job-b",
            "direct",
            schedule,
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        source.save_job_definition(&project_a).unwrap();
        source.save_job_definition(&project_b).unwrap();
        source
            .create_or_get_occurrence(
                project_a.id(),
                project_a.created_at,
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();

        assert_eq!(
            destination.copy_project_from(&source, "project-a").unwrap(),
            1
        );
        let definitions = destination.list_job_definitions().unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].project_id, "project-a");
        assert_eq!(destination.list_occurrences("job-a", 10).unwrap().len(), 1);
        assert!(
            destination
                .list_occurrences("job-b", 10)
                .unwrap()
                .is_empty()
        );
    }
}
