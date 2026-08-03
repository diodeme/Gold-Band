use super::occurrence::{
    ClaimResult, OccurrenceLinks, OccurrenceStatus, OccurrenceTriggerKind, ScheduledError,
    ScheduledErrorCode, ScheduledOccurrence,
};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
const BUSY_TIMEOUT_MILLIS: u64 = 5_000;

#[derive(Debug, Error)]
pub enum SchedulerDatabaseError {
    #[error("scheduler sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("scheduler JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid scheduler value: {0}")]
    InvalidValue(String),
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
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MILLIS))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        ensure_schema(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

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
        let occurrence = load_occurrence_by_key(&transaction, job_id, scheduled_at, trigger_kind)?
            .ok_or_else(|| {
                SchedulerDatabaseError::InvalidValue(
                    "occurrence was not available after insertion".to_string(),
                )
            })?;
        transaction.commit()?;
        Ok(occurrence)
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
                 task_id = ?4,
                 run_id = ?5,
                 round_id = ?6,
                 attempt_id = ?7,
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
                links.task_id,
                links.run_id,
                links.round_id,
                links.attempt_id,
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
             WHERE status = 'running' AND lease_until IS NOT NULL AND lease_until <= ?1",
            params![timestamp_millis(now)],
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn mark_missed(&self, job_id: &str, scheduled_at: DateTime<Utc>) -> Result<bool> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now();
        ensure_job_row(&transaction, job_id, now)?;
        let occurrence = load_occurrence_by_key(
            &transaction,
            job_id,
            scheduled_at,
            OccurrenceTriggerKind::Scheduled,
        )?;
        if occurrence.is_none() {
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
        transaction.commit()?;
        Ok(updated == 1)
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

    pub fn schema_version(&self) -> Result<i64> {
        let connection = self.lock_connection()?;
        Ok(
            connection.query_row("SELECT version FROM scheduler_schema LIMIT 1", [], |row| {
                row.get(0)
            })?,
        )
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| {
            SchedulerDatabaseError::InvalidValue("scheduler database lock poisoned".to_string())
        })
    }
}

fn ensure_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
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
    connection.execute(
        "UPDATE scheduler_schema SET version = ?1 WHERE id = 1",
        params![SCHEMA_VERSION],
    )?;
    Ok(())
}

fn ensure_job_row(transaction: &Transaction<'_>, job_id: &str, now: DateTime<Utc>) -> Result<()> {
    transaction.execute(
        "INSERT OR IGNORE INTO scheduled_jobs (id, created_at, updated_at)
         VALUES (?1, ?2, ?2)",
        params![job_id, timestamp_millis(now)],
    )?;
    Ok(())
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
    use super::ScheduledTaskDatabase;
    use crate::scheduler::occurrence::{ClaimResult, OccurrenceStatus, OccurrenceTriggerKind};
    use camino::Utf8PathBuf;
    use chrono::{Duration, TimeZone, Utc};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn database() -> (tempfile::TempDir, ScheduledTaskDatabase) {
        let temp = tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(temp.path().join("scheduled-tasks.db")).unwrap();
        let database = ScheduledTaskDatabase::open(db_path).unwrap();
        (temp, database)
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
        assert_eq!(
            [first_result.is_claimed(), second_result.is_claimed()],
            [true, false]
        );
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
}
