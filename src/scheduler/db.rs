use super::ScheduledTaskDefinition;
use super::occurrence::{
    ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind, ScheduledError,
    ScheduledErrorCode, ScheduledOccurrence,
};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT_MILLIS: u64 = 3_000;
const SCHEMA_COMPONENT: &str = "scheduler";
pub const OCCURRENCE_HISTORY_PAGE_SIZE: usize = 20;

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OccurrencePageCursor {
    pub scheduled_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrencePage {
    pub items: Vec<ScheduledOccurrence>,
    pub next_cursor: Option<OccurrencePageCursor>,
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

#[derive(Debug, Error)]
pub enum SchedulerDatabaseError {
    #[error("scheduler sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("scheduler storage error: {0}")]
    Io(#[from] std::io::Error),
    #[error("scheduler JSON error: {0}")]
    Json(#[from] serde_json::Error),
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
             PRAGMA synchronous = FULL;",
        )?;
        ensure_schema(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
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

    pub fn list_enabled_jobs(&self, project_id: &str) -> Result<Vec<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT project_id, definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE project_id = ?1 AND enabled = 1 AND definition_json IS NOT NULL
             ORDER BY next_run_at ASC, created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(params![project_id], map_job_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn list_recoverable_jobs_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<RecoverableScheduledJob>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT job.project_id,
                    job.definition_json,
                    job.revision,
                    job.next_run_at,
                    EXISTS(
                        SELECT 1 FROM scheduled_occurrences AS occurrence
                        WHERE occurrence.project_id = job.project_id
                          AND occurrence.job_id = job.id
                          AND occurrence.status IN ('pending', 'retrying')
                    ),
                    (
                        SELECT MIN(occurrence.lease_until)
                        FROM scheduled_occurrences AS occurrence
                        WHERE occurrence.project_id = job.project_id
                          AND occurrence.job_id = job.id
                          AND occurrence.status = 'running'
                    )
             FROM scheduled_jobs AS job
             WHERE job.project_id = ?1
               AND job.definition_json IS NOT NULL
               AND (
                   job.enabled = 1
                   OR EXISTS(
                       SELECT 1 FROM scheduled_occurrences AS occurrence
                       WHERE occurrence.project_id = job.project_id
                         AND occurrence.job_id = job.id
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
                "SELECT job.project_id,
                        job.definition_json,
                        job.revision,
                        job.next_run_at,
                        EXISTS(
                            SELECT 1 FROM scheduled_occurrences AS occurrence
                            WHERE occurrence.project_id = job.project_id
                              AND occurrence.job_id = job.id
                              AND occurrence.status IN ('pending', 'retrying')
                        ),
                        (
                            SELECT MIN(occurrence.lease_until)
                            FROM scheduled_occurrences AS occurrence
                            WHERE occurrence.project_id = job.project_id
                              AND occurrence.job_id = job.id
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
                           WHERE occurrence.project_id = job.project_id
                             AND occurrence.job_id = job.id
                             AND occurrence.status IN ('pending', 'retrying', 'running')
                       )
                   )",
                params![project_id, job_id],
                map_recoverable_job,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn enabled_job_count(&self, project_id: &str) -> Result<usize> {
        let connection = self.lock_connection()?;
        let count = connection.query_row(
            "SELECT COUNT(*) FROM scheduled_jobs
             WHERE project_id = ?1 AND enabled = 1 AND definition_json IS NOT NULL",
            params![project_id],
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
             ON CONFLICT(project_id, id) DO UPDATE SET
                 enabled = excluded.enabled,
                 definition_json = excluded.definition_json,
                 created_at = excluded.created_at,
                 updated_at = excluded.updated_at,
                 revision = scheduled_jobs.revision + 1
             WHERE scheduled_jobs.enabled IS NOT excluded.enabled
                OR scheduled_jobs.definition_json IS NOT excluded.definition_json
                OR scheduled_jobs.created_at IS NOT excluded.created_at
                OR scheduled_jobs.updated_at IS NOT excluded.updated_at",
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
        let deleted = transaction.execute(
            "DELETE FROM scheduled_jobs WHERE project_id = ?1 AND id = ?2",
            params![project_id, job_id],
        )?;
        transaction.commit()?;
        Ok(deleted == 1)
    }

    pub fn scan_job_definitions(&self, project_id: &str) -> Result<ScheduledJobDefinitionScan> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT project_id, definition_json
             FROM scheduled_jobs
             WHERE project_id = ?1 AND definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut definitions = Vec::new();
        let mut invalid_count = 0;
        for row in rows {
            match row.and_then(|(stored_project_id, definition_json)| {
                let definition: ScheduledTaskDefinition = serde_json::from_str(&definition_json)
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                if definition.project_id != stored_project_id {
                    return Err(to_conversion_error(format!(
                        "scheduled job project_id mismatch: row={stored_project_id}, definition={}",
                        definition.project_id
                    )));
                }
                Ok(definition)
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
            "SELECT project_id, definition_json
             FROM scheduled_jobs
             WHERE project_id = ?1 AND definition_json IS NOT NULL
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(params![project_id], |row| {
            let stored_project_id = row.get::<_, String>(0)?;
            let definition =
                serde_json::from_str::<ScheduledTaskDefinition>(&row.get::<_, String>(1)?)
                    .map_err(to_conversion_error)?;
            if definition.project_id != stored_project_id {
                return Err(to_conversion_error(format!(
                    "scheduled job project_id mismatch: row={stored_project_id}, definition={}",
                    definition.project_id
                )));
            }
            Ok(definition)
        })?;
        let mut definitions = Vec::new();
        for row in rows {
            definitions.push(row?);
        }
        Ok(definitions)
    }

    pub fn list_job_records_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<ScheduledJobRecord>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT project_id, definition_json, revision, next_run_at
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
            project_id,
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
        project_id: &str,
        job_id: &str,
        scheduled_at: DateTime<Utc>,
        trigger_kind: OccurrenceTriggerKind,
    ) -> Result<ScheduledOccurrence> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        ensure_job_row(&transaction, project_id, job_id, now)?;
        let occurrence = insert_or_get_occurrence_tx(
            &transaction,
            project_id,
            job_id,
            scheduled_at,
            trigger_kind,
            now,
        )?;
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
                 project_id, id, job_id, scheduled_at, trigger_kind, status, attempt,
                 created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'scheduled', 'pending', 0, ?5, ?5)",
            params![
                project_id,
                format!("occurrence-{}", Uuid::new_v4()),
                job_id,
                timestamp_millis(deadline),
                timestamp_millis(now),
            ],
        )?;
        let occurrence = load_occurrence_by_key(
            &transaction,
            project_id,
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

    pub fn get_occurrence(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        load_occurrence_by_id(&connection, project_id, id)
    }

    pub fn claim_occurrence(
        &self,
        project_id: &str,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimResult> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((status, current_owner, current_lease)) =
            occurrence_claim_state(&transaction, project_id, id)?
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
                 owner_id = ?3,
                 lease_until = ?4,
                 heartbeat_at = ?5,
                 started_at = COALESCE(started_at, ?5),
                 finished_at = NULL,
                 error_code = NULL,
                 error_params = NULL,
                 updated_at = ?5
             WHERE project_id = ?1 AND id = ?2
               AND (
                   status IN ('pending', 'retrying')
                   OR (status = 'running' AND lease_until IS NOT NULL AND lease_until <= ?6)
               )",
            params![
                project_id,
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

        let occurrence =
            load_occurrence_by_id_tx(&transaction, project_id, id)?.ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue("claimed occurrence disappeared".to_string())
            })?;
        transaction.commit()?;
        Ok(ClaimResult::Claimed(occurrence))
    }

    pub fn resume_attention_occurrence(
        &self,
        project_id: &str,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<ClaimResult> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some((status, _, _)) = occurrence_claim_state(&transaction, project_id, id)? else {
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
                 owner_id = ?3,
                 lease_until = ?4,
                 heartbeat_at = ?5,
                 started_at = COALESCE(started_at, ?5),
                 finished_at = NULL,
                 error_code = NULL,
                 error_params = NULL,
                 updated_at = ?5
             WHERE project_id = ?1 AND id = ?2 AND status = 'attention_required'",
            params![
                project_id,
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
        let occurrence =
            load_occurrence_by_id_tx(&transaction, project_id, id)?.ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue(
                    "resumed attention occurrence disappeared".to_string(),
                )
            })?;
        transaction.commit()?;
        Ok(ClaimResult::Claimed(occurrence))
    }

    pub fn find_attention_occurrence_by_links(
        &self,
        project_id: &str,
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
                 WHERE project_id = ?1
                   AND status = 'attention_required'
                   AND task_id = ?2
                   AND run_id = ?3
                   AND round_id = ?4
                   AND attempt_id = ?5
                 ORDER BY updated_at DESC
                 LIMIT 1",
                params![project_id, task_id, run_id, round_id, attempt_id],
                map_occurrence,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn renew_lease(
        &self,
        project_id: &str,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET lease_until = ?4, heartbeat_at = ?5, updated_at = ?5
             WHERE project_id = ?1 AND id = ?2
               AND owner_id = ?3
               AND status = 'running'
               AND lease_until IS NOT NULL
               AND lease_until > ?5",
            params![
                project_id,
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
        project_id: &str,
        id: &str,
        owner_id: &str,
        now: DateTime<Utc>,
        links: &OccurrenceLinks,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let updated = transaction.execute(
            "UPDATE scheduled_occurrences
             SET task_id = COALESCE(task_id, ?5),
                 run_id = COALESCE(run_id, ?6),
                 round_id = COALESCE(round_id, ?7),
                 attempt_id = COALESCE(attempt_id, ?8),
                 updated_at = ?4
             WHERE project_id = ?1 AND id = ?2
               AND owner_id = ?3
               AND status = 'running'
               AND lease_until IS NOT NULL
               AND lease_until > ?4
               AND (task_id IS NULL OR ?5 IS NULL OR task_id = ?5)
               AND (run_id IS NULL OR ?6 IS NULL OR run_id = ?6)
               AND (round_id IS NULL OR ?7 IS NULL OR round_id = ?7)
               AND (attempt_id IS NULL OR ?8 IS NULL OR attempt_id = ?8)",
            params![
                project_id,
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
        project_id: &str,
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
                 updated_at = ?4
             WHERE project_id = ?1 AND id = ?2 AND owner_id = ?3 AND status = 'running'",
            params![project_id, id, owner_id, timestamp_millis(now)],
        )?;
        transaction.commit()?;
        Ok(updated == 1)
    }

    pub fn finish_occurrence(
        &self,
        project_id: &str,
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
             SET status = ?4,
                 owner_id = NULL,
                 lease_until = NULL,
                 heartbeat_at = NULL,
                  task_id = COALESCE(?5, task_id),
                  run_id = COALESCE(?6, run_id),
                  round_id = COALESCE(?7, round_id),
                  attempt_id = COALESCE(?8, attempt_id),
                 error_code = ?9,
                 error_params = ?10,
                 finished_at = ?11,
                 updated_at = ?12
             WHERE project_id = ?1 AND id = ?2
               AND owner_id = ?3
               AND status = 'running'
               AND (lease_until IS NULL OR lease_until >= ?12)",
            params![
                project_id,
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

    pub fn recover_expired(&self, project_id: &str, now: DateTime<Utc>) -> Result<usize> {
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
                 updated_at = ?2
             WHERE project_id = ?1
               AND status = 'running' AND (lease_until IS NULL OR lease_until <= ?2)",
            params![project_id, timestamp_millis(now)],
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
        let updated = mark_missed_tx(&transaction, project_id, job_id, scheduled_at, Utc::now())?;
        transaction.commit()?;
        Ok(Some(updated))
    }

    #[cfg(test)]
    pub fn mark_missed(
        &self,
        project_id: &str,
        job_id: &str,
        scheduled_at: DateTime<Utc>,
    ) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        ensure_job_row(&transaction, project_id, job_id, now)?;
        let updated = mark_missed_tx(&transaction, project_id, job_id, scheduled_at, now)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn list_occurrences(
        &self,
        project_id: &str,
        job_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences
             WHERE project_id = ?1 AND job_id = ?2
             ORDER BY scheduled_at DESC, created_at DESC
             LIMIT ?3",
        )?;
        let rows =
            statement.query_map(params![project_id, job_id, limit as i64], map_occurrence)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn count_run_occurrences(&self, project_id: &str, job_id: &str) -> Result<u64> {
        let connection = self.lock_connection()?;
        let count = connection.query_row(
            "SELECT COUNT(*) FROM scheduled_occurrences
             WHERE project_id = ?1 AND job_id = ?2 AND run_id IS NOT NULL",
            params![project_id, job_id],
            |row| row.get::<_, i64>(0),
        )?;
        u64::try_from(count).map_err(|_| {
            SchedulerDatabaseError::InvalidValue("occurrence run count is out of range".to_string())
        })
    }

    pub fn list_occurrence_page(
        &self,
        project_id: &str,
        job_id: &str,
        status: Option<OccurrenceStatus>,
        cursor: Option<&OccurrencePageCursor>,
        page_size: usize,
    ) -> Result<OccurrencePage> {
        if page_size == 0 {
            return Err(SchedulerDatabaseError::InvalidValue(
                "occurrence page size must be greater than zero".to_string(),
            ));
        }
        let fetch_size = page_size.checked_add(1).ok_or_else(|| {
            SchedulerDatabaseError::InvalidValue("occurrence page size is out of range".to_string())
        })?;
        let fetch_size = i64::try_from(fetch_size).map_err(|_| {
            SchedulerDatabaseError::InvalidValue("occurrence page size is out of range".to_string())
        })?;
        let connection = self.lock_connection()?;
        let cursor_scheduled_at = cursor.map(|value| timestamp_millis(value.scheduled_at));
        let cursor_created_at = cursor.map(|value| timestamp_millis(value.created_at));
        let cursor_id = cursor.map(|value| value.id.as_str());
        let mut items = if let Some(status) = status {
            let mut statement = connection.prepare(
                "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                        owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                        error_code, error_params, started_at, finished_at, created_at, updated_at
                 FROM scheduled_occurrences
                 WHERE project_id = ?1 AND job_id = ?2 AND status = ?3
                   AND (?4 IS NULL OR scheduled_at < ?4
                        OR (scheduled_at = ?4 AND created_at < ?5)
                        OR (scheduled_at = ?4 AND created_at = ?5 AND id < ?6))
                 ORDER BY scheduled_at DESC, created_at DESC, id DESC
                 LIMIT ?7",
            )?;
            statement
                .query_map(
                    params![
                        project_id,
                        job_id,
                        status.to_string(),
                        cursor_scheduled_at,
                        cursor_created_at,
                        cursor_id,
                        fetch_size
                    ],
                    map_occurrence,
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut statement = connection.prepare(
                "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                        owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                        error_code, error_params, started_at, finished_at, created_at, updated_at
                 FROM scheduled_occurrences
                 WHERE project_id = ?1 AND job_id = ?2
                   AND (?3 IS NULL OR scheduled_at < ?3
                        OR (scheduled_at = ?3 AND created_at < ?4)
                        OR (scheduled_at = ?3 AND created_at = ?4 AND id < ?5))
                 ORDER BY scheduled_at DESC, created_at DESC, id DESC
                 LIMIT ?6",
            )?;
            statement
                .query_map(
                    params![
                        project_id,
                        job_id,
                        cursor_scheduled_at,
                        cursor_created_at,
                        cursor_id,
                        fetch_size
                    ],
                    map_occurrence,
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let has_more = items.len() > page_size;
        if has_more {
            items.truncate(page_size);
        }
        let next_cursor = has_more.then(|| {
            let last = items.last().expect("a page with more rows is non-empty");
            OccurrencePageCursor {
                scheduled_at: last.scheduled_at,
                created_at: last.created_at,
                id: last.id.clone(),
            }
        });
        Ok(OccurrencePage { items, next_cursor })
    }

    /// 列出某个 job 当前处于 running 状态的 occurrence，用于主动状态对账：
    /// 这些 occurrence 理论上有 Task/Run 在执行；若其 Task/Run 已不再 active，
    /// scheduler 应当对账收尾，避免 occurrence 因 lifecycle 事件丢失而永久卡 running。
    pub fn list_running_occurrences_for_job(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> Result<Vec<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        let mut statement = connection.prepare(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences
             WHERE project_id = ?1 AND job_id = ?2 AND status = 'running'
             ORDER BY scheduled_at ASC, created_at ASC",
        )?;
        let rows = statement.query_map(params![project_id, job_id], map_occurrence)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn oldest_runnable_occurrence(
        &self,
        project_id: &str,
        job_id: &str,
    ) -> Result<Option<ScheduledOccurrence>> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                        owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                        error_code, error_params, started_at, finished_at, created_at, updated_at
                 FROM scheduled_occurrences
                 WHERE project_id = ?1 AND job_id = ?2
                   AND status IN ('pending', 'retrying')
                 ORDER BY scheduled_at ASC, created_at ASC
                 LIMIT 1",
                params![project_id, job_id],
                map_occurrence,
            )
            .optional()
            .map_err(SchedulerDatabaseError::from)
    }

    pub fn cleanup_terminal_occurrences(
        &self,
        project_id: &str,
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
             WHERE (project_id, id) IN (
                 SELECT occurrence.project_id, occurrence.id
                 FROM scheduled_occurrences AS occurrence
                 WHERE occurrence.project_id = ?1
                   AND occurrence.status IN ('succeeded', 'failed', 'skipped', 'missed')
                   AND occurrence.finished_at IS NOT NULL
                   AND occurrence.finished_at < ?2
                   AND NOT EXISTS (
                       SELECT 1 FROM scheduler_protected_runs AS protected
                       WHERE protected.run_id = occurrence.run_id
                   )
                 ORDER BY occurrence.finished_at ASC, occurrence.id ASC
                 LIMIT ?3
             )",
            params![project_id, timestamp_millis(cutoff), batch_size],
        )?;
        let has_more = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM scheduled_occurrences AS occurrence
                 WHERE occurrence.project_id = ?1
                   AND occurrence.status IN ('succeeded', 'failed', 'skipped', 'missed')
                   AND occurrence.finished_at IS NOT NULL
                   AND occurrence.finished_at < ?2
                   AND NOT EXISTS (
                       SELECT 1 FROM scheduler_protected_runs AS protected
                       WHERE protected.run_id = occurrence.run_id
                   )
             )",
            params![project_id, timestamp_millis(cutoff)],
            |row| row.get(0),
        )?;
        transaction.execute("DELETE FROM scheduler_protected_runs", [])?;
        transaction.commit()?;
        Ok(RetentionResult { deleted, has_more })
    }

    pub fn schema_version(&self) -> Result<i64> {
        let connection = self.lock_connection()?;
        Ok(connection.query_row(
            "SELECT version FROM core_schema WHERE component = ?1",
            params![SCHEMA_COMPONENT],
            |row| row.get(0),
        )?)
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| {
            SchedulerDatabaseError::InvalidValue("scheduler database lock poisoned".to_string())
        })
    }
}

fn ensure_schema(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS core_schema (
             component TEXT PRIMARY KEY NOT NULL,
             version INTEGER NOT NULL
         );",
    )?;
    let version = connection
        .query_row(
            "SELECT version FROM core_schema WHERE component = ?1",
            params![SCHEMA_COMPONENT],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if version.is_some_and(|found| found > SCHEMA_VERSION) {
        return Err(SchedulerDatabaseError::UnsupportedSchemaVersion {
            found: version.unwrap_or_default(),
            supported: SCHEMA_VERSION,
        });
    }
    if version == Some(SCHEMA_VERSION) {
        return Ok(());
    }
    if version.is_some() {
        return Err(SchedulerDatabaseError::InvalidValue(format!(
            "unsupported scheduler schema version: {}",
            version.unwrap_or_default()
        )));
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    create_schema_v1(&transaction)?;
    transaction.execute(
        "INSERT INTO core_schema(component, version) VALUES (?1, ?2)",
        params![SCHEMA_COMPONENT, SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(())
}

fn create_schema_v1(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "CREATE TABLE scheduled_jobs (
             project_id TEXT NOT NULL,
             id TEXT NOT NULL,
             enabled INTEGER NOT NULL DEFAULT 1,
             definition_json TEXT,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             revision INTEGER NOT NULL DEFAULT 1,
             next_run_at INTEGER,
             PRIMARY KEY (project_id, id)
         );

         CREATE TABLE scheduled_occurrences (
             project_id TEXT NOT NULL,
             id TEXT NOT NULL,
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
             PRIMARY KEY (project_id, id),
             FOREIGN KEY (project_id, job_id)
                 REFERENCES scheduled_jobs(project_id, id)
                 ON DELETE CASCADE,
             UNIQUE (project_id, job_id, scheduled_at, trigger_kind)
         );

         CREATE INDEX idx_scheduled_jobs_enabled_deadline
             ON scheduled_jobs(project_id, enabled, next_run_at)
             WHERE enabled = 1;
         CREATE INDEX idx_scheduled_occurrences_active
             ON scheduled_occurrences(project_id, job_id, scheduled_at)
             WHERE status IN ('pending', 'running', 'retrying');
         CREATE INDEX idx_scheduled_occurrences_history
             ON scheduled_occurrences(
                 project_id, job_id, scheduled_at DESC, created_at DESC, id DESC
             );
         CREATE INDEX idx_scheduled_occurrences_status_history
             ON scheduled_occurrences(
                 project_id, job_id, status, scheduled_at DESC, created_at DESC, id DESC
             );",
    )?;
    Ok(())
}
pub fn derived_next_run_at(definition: &ScheduledTaskDefinition) -> Option<DateTime<Utc>> {
    if !definition.enabled {
        return None;
    }
    // 基准用 last_trigger_at（回退到创建时刻前 1s），保证：
    // - 周期性 schedule（Every/Cron/Repeat）：从上次触发点连续算下一次；
    // - 一次性 At schedule：返回 at 本身（即使已过，legacy 导入历史 job 仍保留触发点）。
    // next_run_at 不会因此倒退——非 schedule 字段的编辑路径会保留 scheduler 已推进的值，
    // 只有 schedule 变化/启停时才用本函数重算。
    let baseline = definition.last_trigger_at.unwrap_or_else(|| {
        definition
            .created_at
            .checked_sub_signed(chrono::Duration::seconds(1))
            .unwrap_or(definition.created_at)
    });
    definition.schedule.next_occurrence_after(baseline)
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
    project_id: &str,
    job_id: &str,
    scheduled_at: DateTime<Utc>,
    trigger_kind: OccurrenceTriggerKind,
    now: DateTime<Utc>,
) -> Result<ScheduledOccurrence> {
    transaction.execute(
        "INSERT OR IGNORE INTO scheduled_occurrences (
             project_id, id, job_id, scheduled_at, trigger_kind, status, attempt,
             created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, ?6)",
        params![
            project_id,
            format!("occurrence-{}", Uuid::new_v4()),
            job_id,
            timestamp_millis(scheduled_at),
            trigger_kind.to_string(),
            timestamp_millis(now),
        ],
    )?;
    load_occurrence_by_key(transaction, project_id, job_id, scheduled_at, trigger_kind)?.ok_or_else(
        || {
            SchedulerDatabaseError::InvalidValue(
                "occurrence was not available after insertion".to_string(),
            )
        },
    )
}

fn mark_missed_tx(
    transaction: &Transaction<'_>,
    project_id: &str,
    job_id: &str,
    scheduled_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<bool> {
    if load_occurrence_by_key(
        transaction,
        project_id,
        job_id,
        scheduled_at,
        OccurrenceTriggerKind::Scheduled,
    )?
    .is_none()
    {
        transaction.execute(
            "INSERT INTO scheduled_occurrences (
                 project_id, id, job_id, scheduled_at, trigger_kind, status, attempt,
                 created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'scheduled', 'pending', 0, ?5, ?5)",
            params![
                project_id,
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
             finished_at = ?4,
             updated_at = ?4
         WHERE project_id = ?1 AND job_id = ?2
           AND scheduled_at = ?3
           AND trigger_kind = 'scheduled'
           AND status IN ('pending', 'retrying')",
        params![
            project_id,
            job_id,
            timestamp_millis(scheduled_at),
            timestamp_millis(now)
        ],
    )?;
    Ok(updated == 1)
}

#[cfg(test)]
fn ensure_job_row(
    transaction: &Transaction<'_>,
    project_id: &str,
    job_id: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO scheduled_jobs (project_id, id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![project_id, job_id, timestamp_millis(now)],
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
            "SELECT project_id, definition_json, revision, next_run_at
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
            "SELECT project_id, definition_json, revision, next_run_at
             FROM scheduled_jobs
             WHERE project_id = ?1 AND id = ?2 AND definition_json IS NOT NULL",
            params![project_id, job_id],
            map_job_record,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn map_job_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledJobRecord> {
    let project_id = row.get::<_, String>(0)?;
    let definition_json = row.get::<_, String>(1)?;
    let definition: ScheduledTaskDefinition =
        serde_json::from_str(&definition_json).map_err(to_conversion_error)?;
    if definition.project_id != project_id {
        return Err(to_conversion_error(format!(
            "scheduled job project_id mismatch: row={project_id}, definition={}",
            definition.project_id
        )));
    }
    let next_run_at = row
        .get::<_, Option<i64>>(3)?
        .map(from_timestamp_millis)
        .transpose()
        .map_err(to_conversion_error)?;
    Ok(ScheduledJobRecord {
        definition,
        revision: row.get(2)?,
        next_run_at,
    })
}

fn map_recoverable_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecoverableScheduledJob> {
    let job = map_job_record(row)?;
    let earliest_running_lease_until = row
        .get::<_, Option<i64>>(5)?
        .map(from_timestamp_millis)
        .transpose()
        .map_err(to_conversion_error)?;
    Ok(RecoverableScheduledJob {
        job,
        has_runnable_occurrence: row.get(4)?,
        earliest_running_lease_until,
    })
}

fn occurrence_claim_state(
    transaction: &Transaction<'_>,
    project_id: &str,
    id: &str,
) -> Result<Option<(String, Option<String>, Option<i64>)>> {
    Ok(transaction
        .query_row(
            "SELECT status, owner_id, lease_until FROM scheduled_occurrences
             WHERE project_id = ?1 AND id = ?2",
            params![project_id, id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

fn load_occurrence_by_id(
    source: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<ScheduledOccurrence>> {
    let mut statement = source.prepare(
        "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                error_code, error_params, started_at, finished_at, created_at, updated_at
         FROM scheduled_occurrences WHERE project_id = ?1 AND id = ?2",
    )?;
    statement
        .query_row(params![project_id, id], map_occurrence)
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn load_occurrence_by_id_tx(
    source: &Transaction<'_>,
    project_id: &str,
    id: &str,
) -> Result<Option<ScheduledOccurrence>> {
    source
        .query_row(
            "SELECT id, job_id, scheduled_at, trigger_kind, status, attempt,
                    owner_id, lease_until, heartbeat_at, task_id, run_id, round_id, attempt_id,
                    error_code, error_params, started_at, finished_at, created_at, updated_at
             FROM scheduled_occurrences WHERE project_id = ?1 AND id = ?2",
            params![project_id, id],
            map_occurrence,
        )
        .optional()
        .map_err(SchedulerDatabaseError::from)
}

fn load_occurrence_by_key(
    transaction: &Transaction<'_>,
    project_id: &str,
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
             WHERE project_id = ?1 AND job_id = ?2
               AND scheduled_at = ?3 AND trigger_kind = ?4",
            params![
                project_id,
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
        DueMaterialization, OCCURRENCE_HISTORY_PAGE_SIZE, ScheduledTaskDatabase, UpdateJobResult,
        derived_next_run_at,
    };
    use crate::scheduler::occurrence::{
        ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind,
    };
    use crate::scheduler::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use camino::Utf8PathBuf;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use rusqlite::{Connection, params};
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    const TEST_PROJECT_ID: &str = "project-a";

    #[derive(Clone)]
    struct ProjectScopedTestDatabase {
        database: ScheduledTaskDatabase,
    }

    impl ProjectScopedTestDatabase {
        fn new(database: ScheduledTaskDatabase) -> Self {
            Self { database }
        }

        fn create_or_get_occurrence(
            &self,
            job_id: &str,
            scheduled_at: DateTime<Utc>,
            trigger_kind: OccurrenceTriggerKind,
        ) -> super::Result<crate::scheduler::occurrence::ScheduledOccurrence> {
            self.database.create_or_get_occurrence(
                TEST_PROJECT_ID,
                job_id,
                scheduled_at,
                trigger_kind,
            )
        }

        fn get_occurrence(
            &self,
            id: &str,
        ) -> super::Result<Option<crate::scheduler::occurrence::ScheduledOccurrence>> {
            self.database.get_occurrence(TEST_PROJECT_ID, id)
        }

        fn claim_occurrence(
            &self,
            id: &str,
            owner_id: &str,
            now: DateTime<Utc>,
            lease_until: DateTime<Utc>,
        ) -> super::Result<ClaimResult> {
            self.database
                .claim_occurrence(TEST_PROJECT_ID, id, owner_id, now, lease_until)
        }

        fn resume_attention_occurrence(
            &self,
            id: &str,
            owner_id: &str,
            now: DateTime<Utc>,
            lease_until: DateTime<Utc>,
        ) -> super::Result<ClaimResult> {
            self.database.resume_attention_occurrence(
                TEST_PROJECT_ID,
                id,
                owner_id,
                now,
                lease_until,
            )
        }

        fn find_attention_occurrence_by_links(
            &self,
            task_id: &str,
            run_id: &str,
            round_id: &str,
            attempt_id: &str,
        ) -> super::Result<Option<crate::scheduler::occurrence::ScheduledOccurrence>> {
            self.database.find_attention_occurrence_by_links(
                TEST_PROJECT_ID,
                task_id,
                run_id,
                round_id,
                attempt_id,
            )
        }

        fn renew_lease(
            &self,
            id: &str,
            owner_id: &str,
            now: DateTime<Utc>,
            lease_until: DateTime<Utc>,
        ) -> super::Result<bool> {
            self.database
                .renew_lease(TEST_PROJECT_ID, id, owner_id, now, lease_until)
        }

        fn accept_occurrence_links(
            &self,
            id: &str,
            owner_id: &str,
            now: DateTime<Utc>,
            links: &OccurrenceLinks,
        ) -> super::Result<bool> {
            self.database
                .accept_occurrence_links(TEST_PROJECT_ID, id, owner_id, now, links)
        }

        fn release_owned_occurrence_for_retry(
            &self,
            id: &str,
            owner_id: &str,
            now: DateTime<Utc>,
        ) -> super::Result<bool> {
            self.database
                .release_owned_occurrence_for_retry(TEST_PROJECT_ID, id, owner_id, now)
        }

        fn finish_occurrence(
            &self,
            id: &str,
            owner_id: &str,
            status: OccurrenceStatus,
            links: Option<OccurrenceLinks>,
            error: Option<crate::scheduler::occurrence::ScheduledError>,
        ) -> super::Result<bool> {
            self.database
                .finish_occurrence(TEST_PROJECT_ID, id, owner_id, status, links, error)
        }

        fn recover_expired(&self, now: DateTime<Utc>) -> super::Result<usize> {
            self.database.recover_expired(TEST_PROJECT_ID, now)
        }

        fn mark_missed(&self, job_id: &str, scheduled_at: DateTime<Utc>) -> super::Result<bool> {
            self.database
                .mark_missed(TEST_PROJECT_ID, job_id, scheduled_at)
        }

        fn list_occurrences(
            &self,
            job_id: &str,
            limit: usize,
        ) -> super::Result<Vec<crate::scheduler::occurrence::ScheduledOccurrence>> {
            self.database
                .list_occurrences(TEST_PROJECT_ID, job_id, limit)
        }

        fn list_occurrence_page(
            &self,
            job_id: &str,
            status: Option<OccurrenceStatus>,
            cursor: Option<&super::OccurrencePageCursor>,
            page_size: usize,
        ) -> super::Result<super::OccurrencePage> {
            self.database
                .list_occurrence_page(TEST_PROJECT_ID, job_id, status, cursor, page_size)
        }

        fn list_running_occurrences_for_job(
            &self,
            job_id: &str,
        ) -> super::Result<Vec<crate::scheduler::occurrence::ScheduledOccurrence>> {
            self.database
                .list_running_occurrences_for_job(TEST_PROJECT_ID, job_id)
        }

        fn oldest_runnable_occurrence(
            &self,
            job_id: &str,
        ) -> super::Result<Option<crate::scheduler::occurrence::ScheduledOccurrence>> {
            self.database
                .oldest_runnable_occurrence(TEST_PROJECT_ID, job_id)
        }

        fn cleanup_terminal_occurrences(
            &self,
            cutoff: DateTime<Utc>,
            batch_size: usize,
            protected_run_ids: &HashSet<String>,
        ) -> super::Result<super::RetentionResult> {
            self.database.cleanup_terminal_occurrences(
                TEST_PROJECT_ID,
                cutoff,
                batch_size,
                protected_run_ids,
            )
        }

        fn enabled_job_count(&self) -> super::Result<usize> {
            self.database.enabled_job_count(TEST_PROJECT_ID)
        }

        fn list_enabled_jobs(&self) -> super::Result<Vec<super::ScheduledJobRecord>> {
            self.database.list_enabled_jobs(TEST_PROJECT_ID)
        }
    }

    impl std::ops::Deref for ProjectScopedTestDatabase {
        type Target = ScheduledTaskDatabase;

        fn deref(&self) -> &Self::Target {
            &self.database
        }
    }

    fn database() -> (tempfile::TempDir, ProjectScopedTestDatabase) {
        let temp = tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(temp.path().join("scheduled-tasks.db")).unwrap();
        let database = ScheduledTaskDatabase::open(db_path).unwrap();
        (temp, ProjectScopedTestDatabase::new(database))
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

        let scan = database.scan_job_definitions("project-a").unwrap();

        assert_eq!(scan.definitions, vec![valid]);
        assert_eq!(scan.invalid_count, 1);
        assert!(
            database
                .list_job_definitions_for_project("project-a")
                .is_err()
        );
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
                 SET status = ?3, finished_at = ?4, run_id = ?5, updated_at = ?4
                 WHERE project_id = ?1 AND id = ?2",
                params![
                    TEST_PROJECT_ID,
                    occurrence_id,
                    status,
                    finished_at.timestamp_millis(),
                    run_id
                ],
            )
            .unwrap();
    }

    #[test]
    fn reopening_schema_v1_does_not_execute_schema_ddl() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("scheduled-tasks.db");
        let database = ScheduledTaskDatabase::open(&path).unwrap();
        let schema_revision = database
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        drop(database);

        let reopened = ScheduledTaskDatabase::open(&path).unwrap();
        let reopened_schema_revision = reopened
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA schema_version", [], |row| row.get::<_, i64>(0))
            .unwrap();

        assert_eq!(reopened.schema_version().unwrap(), 1);
        assert_eq!(reopened_schema_revision, schema_revision);
    }

    #[test]
    fn newer_schema_version_is_rejected_without_downgrade() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("scheduled-tasks.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE core_schema (
                     component TEXT PRIMARY KEY NOT NULL, version INTEGER NOT NULL
                 );
                 INSERT INTO core_schema(component, version) VALUES ('scheduler', 2);",
            )
            .unwrap();
        drop(connection);

        assert!(matches!(
            ScheduledTaskDatabase::open(&path),
            Err(super::SchedulerDatabaseError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            })
        ));
        let connection = Connection::open(&path).unwrap();
        let version = connection
            .query_row(
                "SELECT version FROM core_schema WHERE component = 'scheduler'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(version, 2);
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
    fn due_materialization_advances_a_millisecond_persisted_every_deadline_once() {
        let (_temp, database) = database();
        let anchor = DateTime::parse_from_rfc3339("2026-08-12T07:16:49.249706300Z")
            .unwrap()
            .with_timezone(&Utc);
        let deadline = DateTime::parse_from_rfc3339("2026-08-12T08:22:49.249Z")
            .unwrap()
            .with_timezone(&Utc);
        let definition = definition(
            "project-a",
            "job-millisecond-deadline",
            ScheduleSpec::every(3, "minutes", anchor).unwrap(),
        );
        let created = database.create_job(&definition, Some(deadline)).unwrap();

        let DueMaterialization::Ready { job, occurrence } = database
            .materialize_due_occurrence(
                &definition.project_id,
                definition.id(),
                created.revision,
                deadline,
            )
            .unwrap()
        else {
            panic!("expected a due occurrence");
        };

        assert_eq!(occurrence.scheduled_at, deadline);
        assert_eq!(
            job.next_run_at,
            Some(
                DateTime::parse_from_rfc3339("2026-08-12T08:25:49.249Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
        assert_eq!(job.revision, created.revision + 1);
        assert!(matches!(
            database
                .materialize_due_occurrence(
                    &definition.project_id,
                    definition.id(),
                    job.revision,
                    deadline,
                )
                .unwrap(),
            DueMaterialization::NotDue
        ));
        let stored = database
            .get_job_definition(&definition.project_id, definition.id())
            .unwrap()
            .unwrap();
        assert_eq!(stored.revision, job.revision);
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
        let first_database =
            ProjectScopedTestDatabase::new(ScheduledTaskDatabase::open(&db_path).unwrap());
        let second_database =
            ProjectScopedTestDatabase::new(ScheduledTaskDatabase::open(&db_path).unwrap());
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
                             project_id, id, job_id, scheduled_at, trigger_kind, status, attempt,
                             finished_at, created_at, updated_at
                         ) VALUES (?1, ?2, 'job-1', ?3, 'manual', 'succeeded', 1, ?3, ?3, ?3)",
                        params![
                            TEST_PROJECT_ID,
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
    fn saving_an_unchanged_definition_is_revision_idempotent() {
        let (_temp, database) = database();
        let definition = definition(
            "project-a",
            "job-idempotent-save",
            ScheduleSpec::every(3, "minutes", fixed_time()).unwrap(),
        );

        database.save_job_definition(&definition).unwrap();
        let first = database
            .get_job_definition(&definition.project_id, definition.id())
            .unwrap()
            .unwrap();
        database.save_job_definition(&definition).unwrap();
        let second = database
            .get_job_definition(&definition.project_id, definition.id())
            .unwrap()
            .unwrap();

        assert_eq!(second.revision, first.revision);
        assert_eq!(second.definition, first.definition);
    }

    #[test]
    fn derived_next_run_at_for_every_schedule_always_yields_future_point() {
        let now = Utc::now();
        // 「每 3 分钟」周期性 schedule，last_trigger_at 故意设为很早的过去值：
        // derived 仍应给出一个不早于 now 的未来触发点（Every 总是从 anchor 向前推进到下一个周期）。
        let mut def = definition(
            "project-a",
            "job-derived",
            ScheduleSpec::every(3, "minutes", now).unwrap(),
        );
        def.last_trigger_at = Some(now - Duration::days(1));

        let derived = derived_next_run_at(&def).expect("enabled every job derives a next run at");
        assert!(
            derived >= now,
            "derived next_run_at for every schedule must not be in the past"
        );
    }

    #[test]
    fn derived_next_run_at_disabled_returns_none() {
        let now = Utc::now();
        let mut def = definition(
            "project-a",
            "job-disabled",
            ScheduleSpec::every(3, "minutes", now).unwrap(),
        );
        def.enabled = false;
        assert!(derived_next_run_at(&def).is_none());
    }

    #[test]
    fn list_running_occurrences_for_job_returns_only_running() {
        let (_temp, database) = database();
        let deadline = fixed_time() - Duration::minutes(10);
        let def = definition(
            "project-a",
            "job-running",
            ScheduleSpec::every(1, "hours", deadline).unwrap(),
        );
        let created = database.create_job(&def, Some(deadline)).unwrap();
        let owner = "owner-a";

        // 两条 occurrence：一条 running（有 task_id/run_id），一条 succeeded（终态）。
        let running = database
            .materialize_due_occurrence("project-a", "job-running", created.revision, fixed_time())
            .unwrap();
        let running = match running {
            DueMaterialization::Ready { occurrence, .. } => occurrence,
            other => panic!("unexpected materialization: {other:?}"),
        };
        database
            .claim_occurrence(
                &running.id,
                owner,
                fixed_time(),
                fixed_time() + Duration::minutes(1),
            )
            .unwrap();
        let _succeeded = database
            .create_or_get_occurrence(
                "job-running",
                fixed_time() + Duration::hours(1),
                OccurrenceTriggerKind::Scheduled,
            )
            .unwrap();

        let listed = database
            .list_running_occurrences_for_job("job-running")
            .unwrap();
        assert_eq!(
            listed.len(),
            1,
            "only the running occurrence should be listed"
        );
        assert_eq!(listed[0].id, running.id);
        assert_eq!(listed[0].status, OccurrenceStatus::Running);
    }

    #[test]
    fn occurrence_history_pages_are_stable_and_non_overlapping() {
        let (_temp, database) = database();
        let now = fixed_time();
        let def = definition(
            "project-a",
            "job-paged",
            ScheduleSpec::every(1, "hours", now).unwrap(),
        );
        database.create_job(&def, Some(now)).unwrap();
        for offset in 0..25 {
            let scheduled_at = now + Duration::minutes(offset);
            let occurrence = database
                .create_or_get_occurrence_for_existing_job(
                    "project-a",
                    "job-paged",
                    scheduled_at,
                    OccurrenceTriggerKind::Manual,
                )
                .unwrap()
                .unwrap();
            let owner = format!("owner-{offset}");
            let claim_now = Utc::now();
            let claimed = match database
                .claim_occurrence(
                    &occurrence.id,
                    &owner,
                    claim_now,
                    claim_now + Duration::minutes(5),
                )
                .unwrap()
            {
                ClaimResult::Claimed(claimed) => claimed,
                other => panic!("expected claim, got {other:?}"),
            };
            assert!(
                database
                    .finish_occurrence(
                        &claimed.id,
                        &owner,
                        OccurrenceStatus::Succeeded,
                        None,
                        None,
                    )
                    .unwrap()
            );
        }

        let first = database
            .list_occurrence_page("job-paged", None, None, 20)
            .unwrap();
        assert_eq!(first.items.len(), 20);
        let cursor = first.next_cursor.expect("first page must have a cursor");
        let second = database
            .list_occurrence_page("job-paged", None, Some(&cursor), 20)
            .unwrap();
        assert_eq!(second.items.len(), 5);
        assert!(second.next_cursor.is_none());

        let first_ids = first
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        assert!(
            second
                .items
                .iter()
                .all(|item| !first_ids.contains(item.id.as_str()))
        );
        assert!(
            first.items.last().unwrap().scheduled_at > second.items.first().unwrap().scheduled_at
        );
    }

    #[test]
    fn occurrence_history_status_filter_is_applied_before_paging() {
        let (_temp, database) = database();
        let now = fixed_time();
        let def = definition(
            "project-a",
            "job-filtered",
            ScheduleSpec::every(1, "hours", now).unwrap(),
        );
        database.create_job(&def, Some(now)).unwrap();

        for offset in 0..45 {
            let occurrence = database
                .create_or_get_occurrence_for_existing_job(
                    "project-a",
                    "job-filtered",
                    now + Duration::minutes(offset),
                    OccurrenceTriggerKind::Manual,
                )
                .unwrap()
                .unwrap();
            let owner = format!("filtered-owner-{offset}");
            let claim_now = Utc::now();
            database
                .claim_occurrence(
                    &occurrence.id,
                    &owner,
                    claim_now,
                    claim_now + Duration::minutes(5),
                )
                .unwrap();
            let status = if offset % 2 == 0 {
                OccurrenceStatus::Failed
            } else {
                OccurrenceStatus::Succeeded
            };
            assert!(
                database
                    .finish_occurrence(&occurrence.id, &owner, status, None, None)
                    .unwrap()
            );
        }

        let page = database
            .list_occurrence_page(
                "job-filtered",
                Some(OccurrenceStatus::Failed),
                None,
                OCCURRENCE_HISTORY_PAGE_SIZE,
            )
            .unwrap();
        assert_eq!(page.items.len(), OCCURRENCE_HISTORY_PAGE_SIZE);
        assert!(
            page.items
                .iter()
                .all(|item| item.status == OccurrenceStatus::Failed)
        );
        assert!(page.next_cursor.is_some());
    }

    #[test]
    fn occurrence_history_status_query_uses_the_filtered_history_index() {
        let (_temp, database) = database();
        let connection = database.lock_connection().unwrap();
        let mut statement = connection
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT id FROM scheduled_occurrences
                 WHERE project_id = ?1 AND job_id = ?2 AND status = ?3
                   AND (?4 IS NULL OR scheduled_at < ?4
                        OR (scheduled_at = ?4 AND created_at < ?5)
                        OR (scheduled_at = ?4 AND created_at = ?5 AND id < ?6))
                 ORDER BY scheduled_at DESC, created_at DESC, id DESC
                 LIMIT ?7",
            )
            .unwrap();
        let details = statement
            .query_map(
                params![
                    TEST_PROJECT_ID,
                    "job-a",
                    "failed",
                    None::<i64>,
                    None::<i64>,
                    None::<String>,
                    21
                ],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");

        assert!(
            details.contains("idx_scheduled_occurrences_status_history"),
            "unexpected query plan: {details}"
        );
    }

    #[test]
    fn composite_identity_isolates_identical_job_and_occurrence_ids() {
        let (_temp, database) = database();
        let deadline = fixed_time() + Duration::hours(1);
        let project_a = definition("project-a", "shared-job", ScheduleSpec::at(deadline));
        let project_b = definition("project-b", "shared-job", ScheduleSpec::at(deadline));
        database.create_job(&project_a, Some(deadline)).unwrap();
        database.create_job(&project_b, Some(deadline)).unwrap();

        let created_at = fixed_time().timestamp_millis();
        let connection = database.connection.lock().unwrap();
        for project_id in ["project-a", "project-b"] {
            connection
                .execute(
                    "INSERT INTO scheduled_occurrences (
                         project_id, id, job_id, scheduled_at, trigger_kind,
                         status, attempt, created_at, updated_at
                     ) VALUES (?1, 'shared-occurrence', 'shared-job', ?2,
                               'manual', 'pending', 0, ?2, ?2)",
                    params![project_id, created_at],
                )
                .unwrap();
        }
        drop(connection);

        let now = Utc::now();
        assert!(matches!(
            database
                .database
                .claim_occurrence(
                    "project-a",
                    "shared-occurrence",
                    "owner-a",
                    now,
                    now + Duration::minutes(5),
                )
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        assert_eq!(
            database
                .database
                .get_occurrence("project-b", "shared-occurrence")
                .unwrap()
                .unwrap()
                .status,
            OccurrenceStatus::Pending
        );
        assert!(
            database
                .database
                .finish_occurrence(
                    "project-a",
                    "shared-occurrence",
                    "owner-a",
                    OccurrenceStatus::Succeeded,
                    None,
                    None,
                )
                .unwrap()
        );
        assert!(database.delete_job("project-a", "shared-job").unwrap());
        assert!(
            database
                .database
                .get_occurrence("project-a", "shared-occurrence")
                .unwrap()
                .is_none()
        );
        assert!(
            database
                .database
                .get_occurrence("project-b", "shared-occurrence")
                .unwrap()
                .is_some()
        );
        assert!(
            database
                .get_job_definition("project-b", "shared-job")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn scheduler_rejects_definition_project_mismatch() {
        let (_temp, database) = database();
        let project_a = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        let project_b = definition(
            "project-b",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        database.create_job(&project_a, None).unwrap();
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE scheduled_jobs SET definition_json = ?1
                 WHERE project_id = 'project-a' AND id = 'job-a'",
                params![serde_json::to_string(&project_b).unwrap()],
            )
            .unwrap();

        assert!(database.get_job_definition("project-a", "job-a").is_err());
        let scan = database.scan_job_definitions("project-a").unwrap();
        assert!(scan.definitions.is_empty());
        assert_eq!(scan.invalid_count, 1);
    }

    #[test]
    fn scheduler_and_runtime_recovery_coexist_in_core_database() {
        let temp = tempdir().unwrap();
        let core_path = Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap();
        let core = crate::storage::core_state::CoreStateDatabase::new(core_path.clone());
        core.upsert_runtime_recovery_candidate(
            &crate::storage::core_state::RuntimeRecoveryCandidate::new(
                "C:/workspace-a",
                "project-a",
                "task-a",
                "run-a",
                "token-a",
                "instance-a",
            ),
        )
        .unwrap();

        let scheduler = ScheduledTaskDatabase::open(&core_path).unwrap();
        let job = definition(
            "project-a",
            "job-a",
            ScheduleSpec::at(fixed_time() + Duration::hours(1)),
        );
        scheduler.create_job(&job, None).unwrap();

        assert_eq!(scheduler.schema_version().unwrap(), 1);
        assert_eq!(core.list_runtime_recovery_candidates().unwrap().len(), 1);
        let connection = Connection::open(core_path).unwrap();
        let components = connection
            .prepare("SELECT component FROM core_schema ORDER BY component")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(components, vec!["core", "scheduler"]);
    }

    #[test]
    fn orphaned_legacy_scheduler_file_is_never_opened() {
        let temp = tempdir().unwrap();
        let legacy_path = temp.path().join("scheduled-tasks.db");
        std::fs::write(&legacy_path, b"not a sqlite database").unwrap();
        let core_path = temp.path().join("core.db");

        let database = ScheduledTaskDatabase::open(&core_path).unwrap();

        assert_eq!(database.schema_version().unwrap(), 1);
        assert_eq!(
            std::fs::read(legacy_path).unwrap(),
            b"not a sqlite database"
        );
    }
}
