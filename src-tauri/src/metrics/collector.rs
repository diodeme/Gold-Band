use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use gold_band::app::observability::{
    ExecutionKind, LifecycleEventType, MetricsCounters, MetricsExecutionTrigger,
    MetricsInterventionKind, MetricsPauseReason, MetricsSessionMode, MetricsSubject,
    MetricsTaskOrigin, MetricsTransition, ModelUsage, PendingMetricsFact, TaskCodeChanges,
    TerminalReason, TokenUsage, UserExecutionAction,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

const OUTBOX_PENDING: &str = "pending";
const OUTBOX_IN_FLIGHT: &str = "in_flight";
const OUTBOX_ACKED: &str = "acked";
const OUTBOX_REJECTED: &str = "rejected";
const MAX_EVENT_BYTES: usize = 64 * 1024;
const ACK_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;
const REJECTED_RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
pub(super) const MAX_UPLOAD_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WireEventType {
    #[serde(rename = "execution.started")]
    ExecutionStarted,
    #[serde(rename = "execution.completed")]
    ExecutionCompleted,
    #[serde(rename = "execution.paused")]
    ExecutionPaused,
    #[serde(rename = "execution.resumed")]
    ExecutionResumed,
    #[serde(rename = "intervention.requested")]
    InterventionRequested,
    #[serde(rename = "acceptance.completed")]
    AcceptanceCompleted,
}

impl From<LifecycleEventType> for WireEventType {
    fn from(value: LifecycleEventType) -> Self {
        match value {
            LifecycleEventType::ExecutionStarted => Self::ExecutionStarted,
            LifecycleEventType::ExecutionCompleted => Self::ExecutionCompleted,
            LifecycleEventType::ExecutionPaused => Self::ExecutionPaused,
            LifecycleEventType::ExecutionResumed => Self::ExecutionResumed,
            LifecycleEventType::InterventionRequested => Self::InterventionRequested,
            LifecycleEventType::AcceptanceCompleted => Self::AcceptanceCompleted,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedMetricsEvent {
    pub event_id: String,
    pub event_revision: u64,
    pub event_type: WireEventType,
    pub occurred_at: String,
    pub reported_at: String,
    pub project_id: String,
    pub user_id: String,
    pub workspace: String,
    pub client_version: String,
    pub session_mode: MetricsSessionMode,
    pub execution_kind: ExecutionKind,
    pub execution_id: String,
    pub run_id: String,
    pub round_id: String,
    pub task_origin: MetricsTaskOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_trigger: Option<MetricsExecutionTrigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_kind: Option<gold_band::app::observability::UnitKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<gold_band::app::observability::ExecutionOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<TerminalReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_pass: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervention_kind: Option<MetricsInterventionKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<MetricsPauseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_pause_reason: Option<MetricsPauseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_usages: Option<Vec<ModelUsage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<gold_band::app::observability::LifecycleTiming>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counters: Option<MetricsCounters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_changes: Option<TaskCodeChanges>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TaskMetricsState {
    counters: MetricsCounters,
    acceptance_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub event_id: String,
    pub payload_json: String,
    pub attempt_count: u32,
}

#[derive(Debug)]
pub struct ClaimedBatch {
    pub items: Vec<OutboxItem>,
    pub discarded_exhausted_count: usize,
}

#[derive(Debug, Clone)]
pub struct UploadFailureItem {
    pub event_id: String,
    pub attempt_count: u32,
    pub next_attempt_at: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct UploadFailureDisposition {
    pub rescheduled_count: usize,
    pub dropped_count: usize,
}

#[derive(Debug, Clone)]
pub struct RejectedEvent {
    pub event_id: String,
    pub error_code: String,
}

#[derive(Debug, Clone)]
pub struct BatchDisposition {
    pub accepted_event_ids: Vec<String>,
    pub duplicate_event_ids: Vec<String>,
    pub rejected: Vec<RejectedEvent>,
}

pub enum CollectorCommand {
    Fact {
        fact: PendingMetricsFact,
        reported_at: String,
        client_version: String,
        now_epoch_seconds: i64,
    },
    Claim {
        owner: String,
        now_epoch_seconds: i64,
        lease_seconds: i64,
        limit: usize,
        reply: oneshot::Sender<Result<ClaimedBatch>>,
    },
    ApplyDisposition {
        owner: String,
        disposition: BatchDisposition,
        now_epoch_seconds: i64,
        reply: oneshot::Sender<Result<()>>,
    },
    RecordUploadFailure {
        owner: String,
        items: Vec<UploadFailureItem>,
        error_code: String,
        retryable: bool,
        reply: oneshot::Sender<Result<UploadFailureDisposition>>,
    },
    Cleanup {
        now_epoch_seconds: i64,
    },
}

pub fn run_collector_actor(
    database_path: &Path,
    mut receiver: mpsc::Receiver<CollectorCommand>,
    on_error: impl Fn(&'static str, &anyhow::Error),
) -> Result<()> {
    let mut store = MetricsCollectorStore::open(database_path)?;
    while let Some(command) = receiver.blocking_recv() {
        match command {
            CollectorCommand::Fact {
                fact,
                reported_at,
                client_version,
                now_epoch_seconds,
            } => {
                if let Err(error) =
                    store.collect(fact, reported_at, &client_version, now_epoch_seconds)
                {
                    on_error("METRICS_COLLECTOR_STORAGE_FAILED", &error);
                }
            }
            CollectorCommand::Claim {
                owner,
                now_epoch_seconds,
                lease_seconds,
                limit,
                reply,
            } => {
                let _ =
                    reply.send(store.claim_batch(&owner, now_epoch_seconds, lease_seconds, limit));
            }
            CollectorCommand::ApplyDisposition {
                owner,
                disposition,
                now_epoch_seconds,
                reply,
            } => {
                let _ = reply.send(store.apply_disposition(&owner, disposition, now_epoch_seconds));
            }
            CollectorCommand::RecordUploadFailure {
                owner,
                items,
                error_code,
                retryable,
                reply,
            } => {
                let _ =
                    reply.send(store.record_upload_failure(&owner, &items, &error_code, retryable));
            }
            CollectorCommand::Cleanup { now_epoch_seconds } => {
                if let Err(error) = store.cleanup(now_epoch_seconds) {
                    on_error("METRICS_OUTBOX_CLEANUP_FAILED", &error);
                }
            }
        }
    }
    Ok(())
}

pub struct MetricsCollectorStore {
    connection: Connection,
}

impl MetricsCollectorStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create metrics database directory {}", parent.display())
            })?;
        }
        let connection = Connection::open(path)
            .with_context(|| format!("open metrics database {}", path.display()))?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS metrics_task_state (
               project_id TEXT NOT NULL,
               execution_id TEXT NOT NULL,
               last_revision INTEGER NOT NULL,
               state_json TEXT NOT NULL,
               updated_at INTEGER NOT NULL,
               PRIMARY KEY(project_id, execution_id)
             );
             CREATE TABLE IF NOT EXISTS metrics_attempt_state (
               project_id TEXT NOT NULL,
               execution_id TEXT NOT NULL,
               run_id TEXT NOT NULL,
               round_id TEXT NOT NULL,
               node_id TEXT NOT NULL,
               attempt_id TEXT NOT NULL,
               counters_json TEXT NOT NULL,
               updated_at INTEGER NOT NULL,
               PRIMARY KEY(project_id, execution_id, run_id, round_id, node_id, attempt_id)
             );
             CREATE TABLE IF NOT EXISTS metrics_transition_dedup (
               project_id TEXT NOT NULL,
               execution_id TEXT NOT NULL,
               transition_kind TEXT NOT NULL,
               transition_id TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               PRIMARY KEY(project_id, execution_id, transition_kind, transition_id)
             );
             CREATE TABLE IF NOT EXISTS metrics_fact_dedup (
               project_id TEXT NOT NULL,
               execution_id TEXT NOT NULL,
               fact_id TEXT NOT NULL,
               payload_json TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               PRIMARY KEY(project_id, execution_id, fact_id)
             );
             CREATE TABLE IF NOT EXISTS metrics_outbox (
               event_id TEXT PRIMARY KEY,
               project_id TEXT NOT NULL,
               execution_id TEXT NOT NULL,
               event_revision INTEGER NOT NULL,
               reported_at TEXT NOT NULL,
               payload_json TEXT NOT NULL,
               status TEXT NOT NULL CHECK(status IN ('pending','in_flight','acked','rejected')),
               attempt_count INTEGER NOT NULL DEFAULT 0,
               next_attempt_at INTEGER NOT NULL,
               lease_owner TEXT,
               lease_until INTEGER,
               created_at INTEGER NOT NULL,
               acked_at INTEGER,
               last_error_code TEXT,
               UNIQUE(project_id, execution_id, event_revision)
             );
             CREATE INDEX IF NOT EXISTS metrics_outbox_ready
               ON metrics_outbox(status, next_attempt_at, created_at);
             CREATE INDEX IF NOT EXISTS metrics_outbox_lease
               ON metrics_outbox(status, lease_until);",
        )?;
        Ok(Self { connection })
    }

    pub fn collect(
        &mut self,
        fact: PendingMetricsFact,
        reported_at: String,
        client_version: &str,
        now_epoch_seconds: i64,
    ) -> Result<CollectedMetricsEvent> {
        self.collect_with_event_id(
            fact,
            reported_at,
            client_version,
            now_epoch_seconds,
            uuid::Uuid::new_v4().to_string(),
        )
    }

    fn collect_with_event_id(
        &mut self,
        mut fact: PendingMetricsFact,
        reported_at: String,
        client_version: &str,
        now_epoch_seconds: i64,
        event_id: String,
    ) -> Result<CollectedMetricsEvent> {
        fact.validate().map_err(|message| anyhow!(message))?;
        let transaction = self.connection.transaction()?;
        let fact_id = fact.fact_id.clone();
        if let Some(payload_json) = transaction
            .query_row(
                "SELECT payload_json FROM metrics_fact_dedup
                 WHERE project_id = ?1 AND execution_id = ?2 AND fact_id = ?3",
                params![fact.key.project_id, fact.key.execution_id, fact.fact_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return serde_json::from_str(&payload_json)
                .context("decode previously collected metrics fact");
        }
        let (last_revision, mut task_state) = load_task_state(&transaction, &fact)?;
        let attempt_key = fact.subject.attempt_key(&fact.runtime_locator);
        let mut attempt_counters = match attempt_key.as_ref() {
            Some(key) => load_attempt_counters(&transaction, &fact, key)?,
            None => MetricsCounters::default(),
        };

        apply_transition(
            &transaction,
            &fact,
            &mut task_state,
            attempt_key.as_ref().map(|_| &mut attempt_counters),
            now_epoch_seconds,
        )?;
        if let MetricsTransition::Acceptance { passed, .. } = fact.transition {
            task_state.acceptance_attempts = task_state.acceptance_attempts.saturating_add(1);
            fact.payload.passed = Some(passed);
            fact.payload.acceptance_attempt = Some(task_state.acceptance_attempts);
            fact.payload.first_pass = Some(passed && task_state.acceptance_attempts == 1);
        }

        let revision = last_revision.saturating_add(1);
        let terminal = fact.event_type == LifecycleEventType::ExecutionCompleted;
        let counters = terminal.then(|| {
            if fact.subject.is_delivery() {
                task_state.counters.clone()
            } else {
                attempt_counters.clone()
            }
        });
        let event = build_collected_event(
            fact,
            event_id,
            revision,
            reported_at,
            client_version,
            counters,
        );
        let payload_json = serde_json::to_string(&event)?;
        let (status, error_code) = if payload_json.len() > MAX_EVENT_BYTES {
            (OUTBOX_REJECTED, Some("REPORT_EVENT_TOO_LARGE"))
        } else {
            (OUTBOX_PENDING, None)
        };

        save_task_state(
            &transaction,
            &event.project_id,
            &event.execution_id,
            revision,
            &task_state,
            now_epoch_seconds,
        )?;
        transaction.execute(
            "INSERT INTO metrics_fact_dedup(
               project_id, execution_id, fact_id, payload_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.project_id,
                event.execution_id,
                fact_id,
                payload_json,
                now_epoch_seconds,
            ],
        )?;
        if let Some(key) = attempt_key {
            if terminal {
                delete_attempt_state(&transaction, &event, &key)?;
            } else {
                save_attempt_state(
                    &transaction,
                    &event,
                    &key,
                    &attempt_counters,
                    now_epoch_seconds,
                )?;
            }
        }
        transaction.execute(
            "INSERT INTO metrics_outbox(
               event_id, project_id, execution_id, event_revision, reported_at, payload_json,
               status, next_attempt_at, created_at, last_error_code
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
            params![
                event.event_id,
                event.project_id,
                event.execution_id,
                revision,
                event.reported_at,
                payload_json,
                status,
                now_epoch_seconds,
                error_code,
            ],
        )?;
        transaction.commit()?;
        Ok(event)
    }

    pub fn claim_batch(
        &mut self,
        owner: &str,
        now_epoch_seconds: i64,
        lease_seconds: i64,
        limit: usize,
    ) -> Result<ClaimedBatch> {
        ensure!(!owner.trim().is_empty(), "lease owner is required");
        let limit = limit.clamp(1, 100) as i64;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE metrics_outbox
             SET status = ?1, lease_owner = NULL, lease_until = NULL
             WHERE status = ?2 AND lease_until <= ?3",
            params![OUTBOX_PENDING, OUTBOX_IN_FLIGHT, now_epoch_seconds],
        )?;
        let discarded_exhausted_count = transaction.execute(
            "DELETE FROM metrics_outbox
             WHERE status = ?1 AND attempt_count >= ?2",
            params![OUTBOX_PENDING, MAX_UPLOAD_ATTEMPTS],
        )?;
        let report_month = transaction
            .query_row(
                "SELECT substr(reported_at, 1, 7) FROM metrics_outbox
                 WHERE status = ?1 AND next_attempt_at <= ?2 AND attempt_count < ?3
                 ORDER BY created_at, event_revision LIMIT 1",
                params![OUTBOX_PENDING, now_epoch_seconds, MAX_UPLOAD_ATTEMPTS],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(report_month) = report_month else {
            transaction.commit()?;
            return Ok(ClaimedBatch {
                items: Vec::new(),
                discarded_exhausted_count,
            });
        };
        let ids = {
            let mut statement = transaction.prepare(
                "SELECT event_id FROM metrics_outbox
                 WHERE status = ?1 AND next_attempt_at <= ?2
                   AND attempt_count < ?3
                   AND substr(reported_at, 1, 7) = ?4
                 ORDER BY created_at, event_revision
                 LIMIT ?5",
            )?;
            statement
                .query_map(
                    params![
                        OUTBOX_PENDING,
                        now_epoch_seconds,
                        MAX_UPLOAD_ATTEMPTS,
                        report_month,
                        limit
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if ids.is_empty() {
            transaction.commit()?;
            return Ok(ClaimedBatch {
                items: Vec::new(),
                discarded_exhausted_count,
            });
        }
        let lease_until = now_epoch_seconds.saturating_add(lease_seconds.max(1));
        for id in &ids {
            let changed = transaction.execute(
                "UPDATE metrics_outbox
                 SET status = ?1, lease_owner = ?2, lease_until = ?3, attempt_count = attempt_count + 1
                 WHERE event_id = ?4 AND status = ?5 AND attempt_count < ?6",
                params![
                    OUTBOX_IN_FLIGHT,
                    owner,
                    lease_until,
                    id,
                    OUTBOX_PENDING,
                    MAX_UPLOAD_ATTEMPTS
                ],
            )?;
            ensure!(changed == 1, "outbox claim changed unexpectedly");
        }
        let mut items = Vec::with_capacity(ids.len());
        {
            let mut statement = transaction.prepare(
                "SELECT event_id, payload_json, attempt_count
                 FROM metrics_outbox WHERE lease_owner = ?1 AND lease_until = ?2
                 ORDER BY created_at, event_revision",
            )?;
            let rows = statement.query_map(params![owner, lease_until], |row| {
                Ok(OutboxItem {
                    event_id: row.get(0)?,
                    payload_json: row.get(1)?,
                    attempt_count: row.get(2)?,
                })
            })?;
            for row in rows {
                let row = row?;
                if ids.contains(&row.event_id) {
                    items.push(row);
                }
            }
        }
        transaction.commit()?;
        Ok(ClaimedBatch {
            items,
            discarded_exhausted_count,
        })
    }

    pub fn apply_disposition(
        &mut self,
        owner: &str,
        disposition: BatchDisposition,
        now_epoch_seconds: i64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for id in disposition
            .accepted_event_ids
            .into_iter()
            .chain(disposition.duplicate_event_ids)
        {
            let changed = transaction.execute(
                "UPDATE metrics_outbox
                 SET status = ?1, acked_at = ?2, lease_owner = NULL, lease_until = NULL,
                     last_error_code = NULL
                 WHERE event_id = ?3 AND status = ?4 AND lease_owner = ?5",
                params![OUTBOX_ACKED, now_epoch_seconds, id, OUTBOX_IN_FLIGHT, owner],
            )?;
            ensure!(
                changed == 1,
                "batch disposition contains an unclaimed event"
            );
        }
        for rejected in disposition.rejected {
            let changed = transaction.execute(
                "UPDATE metrics_outbox
                 SET status = ?1, acked_at = ?2, lease_owner = NULL, lease_until = NULL,
                     last_error_code = ?3
                 WHERE event_id = ?4 AND status = ?5 AND lease_owner = ?6",
                params![
                    OUTBOX_REJECTED,
                    now_epoch_seconds,
                    rejected.error_code,
                    rejected.event_id,
                    OUTBOX_IN_FLIGHT,
                    owner
                ],
            )?;
            ensure!(
                changed == 1,
                "batch disposition contains an unclaimed event"
            );
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn record_upload_failure(
        &mut self,
        owner: &str,
        items: &[UploadFailureItem],
        error_code: &str,
        retryable: bool,
    ) -> Result<UploadFailureDisposition> {
        ensure!(!owner.trim().is_empty(), "lease owner is required");
        let transaction = self.connection.transaction()?;
        let mut disposition = UploadFailureDisposition::default();
        for item in items {
            let changed = if retryable && item.attempt_count < MAX_UPLOAD_ATTEMPTS {
                let changed = transaction.execute(
                    "UPDATE metrics_outbox
                     SET status = ?1, next_attempt_at = ?2, lease_owner = NULL,
                         lease_until = NULL, last_error_code = ?3
                     WHERE event_id = ?4 AND status = ?5 AND lease_owner = ?6
                       AND attempt_count = ?7",
                    params![
                        OUTBOX_PENDING,
                        item.next_attempt_at,
                        error_code,
                        item.event_id,
                        OUTBOX_IN_FLIGHT,
                        owner,
                        item.attempt_count
                    ],
                )?;
                disposition.rescheduled_count += changed;
                changed
            } else {
                let changed = transaction.execute(
                    "DELETE FROM metrics_outbox
                     WHERE event_id = ?1 AND status = ?2 AND lease_owner = ?3
                       AND attempt_count = ?4",
                    params![item.event_id, OUTBOX_IN_FLIGHT, owner, item.attempt_count],
                )?;
                disposition.dropped_count += changed;
                changed
            };
            ensure!(changed == 1, "upload failure contains an unclaimed event");
        }
        transaction.commit()?;
        Ok(disposition)
    }

    pub fn cleanup(&mut self, now_epoch_seconds: i64) -> Result<usize> {
        let ack_before = now_epoch_seconds.saturating_sub(ACK_RETENTION_SECONDS);
        let rejected_before = now_epoch_seconds.saturating_sub(REJECTED_RETENTION_SECONDS);
        Ok(self.connection.execute(
            "DELETE FROM metrics_outbox
             WHERE (status = ?1 AND acked_at < ?2) OR (status = ?3 AND acked_at < ?4)",
            params![OUTBOX_ACKED, ack_before, OUTBOX_REJECTED, rejected_before],
        )?)
    }

    #[cfg(test)]
    fn task_revision(&self, project_id: &str, execution_id: &str) -> Result<u64> {
        Ok(self.connection.query_row(
            "SELECT last_revision FROM metrics_task_state WHERE project_id = ?1 AND execution_id = ?2",
            params![project_id, execution_id],
            |row| row.get(0),
        )?)
    }
}

fn load_task_state(
    transaction: &Transaction<'_>,
    fact: &PendingMetricsFact,
) -> Result<(u64, TaskMetricsState)> {
    let row = transaction
        .query_row(
            "SELECT last_revision, state_json FROM metrics_task_state
             WHERE project_id = ?1 AND execution_id = ?2",
            params![fact.key.project_id, fact.key.execution_id],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    match row {
        Some((revision, json)) => Ok((revision, serde_json::from_str(&json)?)),
        None => Ok((0, TaskMetricsState::default())),
    }
}

fn load_attempt_counters(
    transaction: &Transaction<'_>,
    fact: &PendingMetricsFact,
    key: &gold_band::app::observability::AttemptMetricsKey,
) -> Result<MetricsCounters> {
    let json = transaction
        .query_row(
            "SELECT counters_json FROM metrics_attempt_state
             WHERE project_id = ?1 AND execution_id = ?2 AND run_id = ?3 AND round_id = ?4
               AND node_id = ?5 AND attempt_id = ?6",
            params![
                fact.key.project_id,
                fact.key.execution_id,
                key.run_id,
                key.round_id,
                key.node_id,
                key.attempt_id,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(match json {
        Some(json) => serde_json::from_str(&json)?,
        None => MetricsCounters::default(),
    })
}

fn save_task_state(
    transaction: &Transaction<'_>,
    project_id: &str,
    execution_id: &str,
    revision: u64,
    state: &TaskMetricsState,
    now_epoch_seconds: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO metrics_task_state(project_id, execution_id, last_revision, state_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(project_id, execution_id) DO UPDATE SET
           last_revision = excluded.last_revision,
           state_json = excluded.state_json,
           updated_at = excluded.updated_at",
        params![
            project_id,
            execution_id,
            revision,
            serde_json::to_string(state)?,
            now_epoch_seconds,
        ],
    )?;
    Ok(())
}

fn save_attempt_state(
    transaction: &Transaction<'_>,
    event: &CollectedMetricsEvent,
    key: &gold_band::app::observability::AttemptMetricsKey,
    counters: &MetricsCounters,
    now_epoch_seconds: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO metrics_attempt_state(
           project_id, execution_id, run_id, round_id, node_id, attempt_id, counters_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(project_id, execution_id, run_id, round_id, node_id, attempt_id) DO UPDATE SET
           counters_json = excluded.counters_json,
           updated_at = excluded.updated_at",
        params![
            event.project_id,
            event.execution_id,
            key.run_id,
            key.round_id,
            key.node_id,
            key.attempt_id,
            serde_json::to_string(counters)?,
            now_epoch_seconds,
        ],
    )?;
    Ok(())
}

fn delete_attempt_state(
    transaction: &Transaction<'_>,
    event: &CollectedMetricsEvent,
    key: &gold_band::app::observability::AttemptMetricsKey,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM metrics_attempt_state
         WHERE project_id = ?1 AND execution_id = ?2 AND run_id = ?3 AND round_id = ?4
           AND node_id = ?5 AND attempt_id = ?6",
        params![
            event.project_id,
            event.execution_id,
            key.run_id,
            key.round_id,
            key.node_id,
            key.attempt_id,
        ],
    )?;
    Ok(())
}

fn apply_transition(
    transaction: &Transaction<'_>,
    fact: &PendingMetricsFact,
    task_state: &mut TaskMetricsState,
    attempt_counters: Option<&mut MetricsCounters>,
    now_epoch_seconds: i64,
) -> Result<()> {
    let (kind, id) = match &fact.transition {
        MetricsTransition::None => return Ok(()),
        MetricsTransition::Paused { transition_id } => ("pause", transition_id.as_str()),
        MetricsTransition::Resumed { transition_id, .. } => ("resume", transition_id.as_str()),
        MetricsTransition::PermissionRequested { request_id } => {
            ("permission", request_id.as_str())
        }
        MetricsTransition::ElicitationRequested { request_id } => {
            ("elicitation", request_id.as_str())
        }
        MetricsTransition::FollowUp { action_id } => ("follow-up", action_id.as_str()),
        MetricsTransition::Acceptance { action_id, .. } => ("acceptance", action_id.as_str()),
    };
    ensure!(!id.trim().is_empty(), "metrics transition id is required");
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO metrics_transition_dedup(
           project_id, execution_id, transition_kind, transition_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            fact.key.project_id,
            fact.key.execution_id,
            kind,
            id,
            now_epoch_seconds,
        ],
    )?;
    if inserted == 0 {
        return Ok(());
    }
    let mut attempt_counters = attempt_counters;
    match fact.transition {
        MetricsTransition::Paused { .. } => {
            task_state.counters.pause_count = task_state.counters.pause_count.saturating_add(1);
            if let Some(counters) = attempt_counters.as_mut() {
                counters.pause_count = counters.pause_count.saturating_add(1);
            }
        }
        MetricsTransition::Resumed { action, .. } => {
            task_state.counters.resume_count = task_state.counters.resume_count.saturating_add(1);
            if let Some(counters) = attempt_counters.as_mut() {
                counters.resume_count = counters.resume_count.saturating_add(1);
            }
            if action == UserExecutionAction::ManualContinue {
                task_state.counters.manual_continue_count =
                    task_state.counters.manual_continue_count.saturating_add(1);
                if let Some(counters) = attempt_counters.as_mut() {
                    counters.manual_continue_count =
                        counters.manual_continue_count.saturating_add(1);
                }
            }
            if let MetricsTransition::Resumed {
                follow_up_action_id: Some(_),
                ..
            } = &fact.transition
            {
                task_state.counters.follow_up_count =
                    task_state.counters.follow_up_count.saturating_add(1);
                if let Some(counters) = attempt_counters.as_mut() {
                    counters.follow_up_count = counters.follow_up_count.saturating_add(1);
                }
            }
        }
        MetricsTransition::PermissionRequested { .. } => {
            task_state.counters.permission_request_count = task_state
                .counters
                .permission_request_count
                .saturating_add(1);
            if let Some(counters) = attempt_counters.as_mut() {
                counters.permission_request_count =
                    counters.permission_request_count.saturating_add(1);
            }
        }
        MetricsTransition::ElicitationRequested { .. } => {
            task_state.counters.elicitation_count =
                task_state.counters.elicitation_count.saturating_add(1);
            if let Some(counters) = attempt_counters.as_mut() {
                counters.elicitation_count = counters.elicitation_count.saturating_add(1);
            }
        }
        MetricsTransition::FollowUp { .. } => {
            task_state.counters.follow_up_count =
                task_state.counters.follow_up_count.saturating_add(1);
            if let Some(counters) = attempt_counters.as_mut() {
                counters.follow_up_count = counters.follow_up_count.saturating_add(1);
            }
        }
        MetricsTransition::Acceptance { .. } | MetricsTransition::None => {}
    }
    Ok(())
}

fn build_collected_event(
    fact: PendingMetricsFact,
    event_id: String,
    event_revision: u64,
    reported_at: String,
    client_version: &str,
    counters: Option<MetricsCounters>,
) -> CollectedMetricsEvent {
    let (node_id, attempt_id, attempt_index, round_index, role_name, unit_kind) =
        match fact.subject.clone() {
            MetricsSubject::DirectTurn {
                attempt_id,
                attempt_index,
            } => (
                None,
                Some(attempt_id),
                Some(attempt_index),
                None,
                None,
                None,
            ),
            MetricsSubject::WorkflowRun | MetricsSubject::AutoOuterRun => {
                (None, None, None, None, None, None)
            }
            MetricsSubject::WorkflowNodeAttempt {
                node_id,
                attempt_id,
                attempt_index,
                round_index,
                role_name,
            } => (
                Some(node_id),
                Some(attempt_id),
                Some(attempt_index),
                Some(round_index),
                Some(role_name),
                None,
            ),
            MetricsSubject::AutoUnitAttempt {
                node_id,
                attempt_id,
                attempt_index,
                round_index,
                role_name,
                unit_kind,
            } => (
                Some(node_id),
                Some(attempt_id),
                Some(attempt_index),
                Some(round_index),
                Some(role_name),
                Some(unit_kind),
            ),
        };
    let payload = fact.payload;
    CollectedMetricsEvent {
        event_id,
        event_revision,
        event_type: fact.event_type.into(),
        occurred_at: fact.occurred_at,
        reported_at,
        project_id: fact.key.project_id,
        user_id: fact.user_id,
        workspace: fact.workspace,
        client_version: client_version.to_string(),
        session_mode: fact.session_mode,
        execution_kind: fact.subject.execution_kind(),
        execution_id: fact.key.execution_id,
        run_id: fact.runtime_locator.run_id,
        round_id: fact.runtime_locator.round_id,
        task_origin: fact.task_origin,
        execution_trigger: fact.execution_trigger,
        task_title: payload.task_title,
        node_id,
        attempt_id,
        attempt_index,
        round_index,
        role_name,
        unit_kind,
        child_run_id: payload.child_run_id,
        outcome: payload.outcome,
        terminal_reason: payload.terminal_reason,
        terminal_reason_code: payload.terminal_reason_code,
        failed_attempt_id: payload.failed_attempt_id,
        round_count: payload.round_count,
        passed: payload.passed,
        acceptance_attempt: payload.acceptance_attempt,
        first_pass: payload.first_pass,
        intervention_kind: payload.intervention_kind,
        pause_reason: payload.pause_reason,
        previous_pause_reason: payload.previous_pause_reason,
        provider: payload.provider,
        model: payload.model,
        usage: payload.usage,
        model_usages: payload.model_usages,
        timing: payload.timing,
        counters,
        code_changes: payload.code_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::app::observability::{MetricsRuntimeLocator, MetricsSubject, TaskMetricsKey};
    use tempfile::tempdir;

    fn workflow_fact(
        event_type: LifecycleEventType,
        run_id: &str,
        round_id: &str,
        attempt_id: &str,
    ) -> PendingMetricsFact {
        let mut fact = PendingMetricsFact::new(
            TaskMetricsKey {
                project_id: "project-1".to_string(),
                execution_id: "task-uuid".to_string(),
            },
            event_type,
            "2026-08-20T00:00:00Z".to_string(),
            "user".to_string(),
            "D:/repo".to_string(),
            MetricsSessionMode::Workflow,
            MetricsSubject::WorkflowNodeAttempt {
                node_id: format!("node-{attempt_id}"),
                attempt_id: attempt_id.to_string(),
                attempt_index: 1,
                round_index: 1,
                role_name: "reviewer".to_string(),
            },
            MetricsRuntimeLocator {
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
            },
            MetricsTaskOrigin::User,
            None,
        );
        if event_type == LifecycleEventType::ExecutionCompleted {
            fact.payload.outcome = Some(gold_band::app::observability::ExecutionOutcome::Success);
            fact.payload.terminal_reason = Some(TerminalReason::Completed);
        }
        fact
    }

    #[test]
    fn revision_is_task_scoped_across_runs_rounds_and_reopen() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("metrics.sqlite3");
        {
            let mut store = MetricsCollectorStore::open(&path).unwrap();
            let first = store
                .collect(
                    workflow_fact(
                        LifecycleEventType::ExecutionStarted,
                        "run-001",
                        "round-001",
                        "attempt-1",
                    ),
                    "2026-08-20T00:00:01Z".to_string(),
                    "test",
                    1,
                )
                .unwrap();
            let second = store
                .collect(
                    workflow_fact(
                        LifecycleEventType::ExecutionStarted,
                        "run-001",
                        "round-002",
                        "attempt-2",
                    ),
                    "2026-08-20T00:00:02Z".to_string(),
                    "test",
                    2,
                )
                .unwrap();
            assert_eq!((first.event_revision, second.event_revision), (1, 2));
        }
        let mut reopened = MetricsCollectorStore::open(&path).unwrap();
        let third = reopened
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-002",
                    "round-001",
                    "attempt-3",
                ),
                "2026-08-20T00:00:03Z".to_string(),
                "test",
                3,
            )
            .unwrap();
        assert_eq!(third.event_revision, 3);
    }

    #[test]
    fn attempt_and_task_counters_use_separate_snapshots_without_double_counting() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let mut paused = workflow_fact(
            LifecycleEventType::ExecutionPaused,
            "run-001",
            "round-001",
            "attempt-1",
        );
        paused.transition = MetricsTransition::Paused {
            transition_id: "pause-1".to_string(),
        };
        store
            .collect(
                paused.clone(),
                "2026-08-20T00:00:01Z".to_string(),
                "test",
                1,
            )
            .unwrap();
        store
            .collect(paused, "2026-08-20T00:00:02Z".to_string(), "test", 2)
            .unwrap();
        let mut resumed = workflow_fact(
            LifecycleEventType::ExecutionResumed,
            "run-001",
            "round-001",
            "attempt-1",
        );
        resumed.transition = MetricsTransition::Resumed {
            transition_id: "resume-1".to_string(),
            action: UserExecutionAction::PermissionResponse,
            follow_up_action_id: None,
        };
        store
            .collect(resumed, "2026-08-20T00:00:03Z".to_string(), "test", 3)
            .unwrap();
        let terminal = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionCompleted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:04Z".to_string(),
                "test",
                4,
            )
            .unwrap();
        let counters = terminal.counters.unwrap();
        assert_eq!(counters.pause_count, 1);
        assert_eq!(counters.resume_count, 1);
        assert_eq!(counters.manual_continue_count, 0);

        let mut delivery = PendingMetricsFact::new(
            TaskMetricsKey {
                project_id: "project-1".to_string(),
                execution_id: "task-uuid".to_string(),
            },
            LifecycleEventType::ExecutionCompleted,
            "2026-08-20T00:00:05Z".to_string(),
            "user".to_string(),
            "D:/repo".to_string(),
            MetricsSessionMode::Workflow,
            MetricsSubject::WorkflowRun,
            MetricsRuntimeLocator {
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
            },
            MetricsTaskOrigin::User,
            None,
        );
        delivery.payload.outcome = Some(gold_band::app::observability::ExecutionOutcome::Success);
        delivery.payload.terminal_reason = Some(TerminalReason::Completed);
        let delivery = store
            .collect(delivery, "2026-08-20T00:00:06Z".to_string(), "test", 6)
            .unwrap();
        assert_eq!(delivery.counters.unwrap().pause_count, 1);
    }

    #[test]
    fn repeated_manual_continue_fact_is_idempotent_and_counts_follow_up_once() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let mut resumed = workflow_fact(
            LifecycleEventType::ExecutionResumed,
            "run-001",
            "round-001",
            "attempt-1",
        );
        resumed.fact_id = "continue-action-1".to_string();
        resumed.transition = MetricsTransition::Resumed {
            transition_id: "continue-action-1".to_string(),
            action: UserExecutionAction::ManualContinue,
            follow_up_action_id: Some("continue-action-1".to_string()),
        };

        let first = store
            .collect(
                resumed.clone(),
                "2026-08-20T00:00:01.000".to_string(),
                "test",
                1,
            )
            .unwrap();
        let replay = store
            .collect(resumed, "2026-08-20T00:00:02.000".to_string(), "test", 2)
            .unwrap();
        assert_eq!(replay.event_id, first.event_id);
        assert_eq!(replay.event_revision, first.event_revision);

        let terminal = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionCompleted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:03.000".to_string(),
                "test",
                3,
            )
            .unwrap();
        assert_eq!(terminal.event_revision, 2);
        let counters = terminal.counters.unwrap();
        assert_eq!(counters.resume_count, 1);
        assert_eq!(counters.manual_continue_count, 1);
        assert_eq!(counters.follow_up_count, 1);
        let outbox_count: u64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM metrics_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_count, 2);
    }

    #[test]
    fn one_attempt_can_count_two_manual_resumes_and_one_content_follow_up() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();

        for (index, has_follow_up) in [(1, false), (2, true)] {
            let mut paused = workflow_fact(
                LifecycleEventType::ExecutionPaused,
                "run-001",
                "round-001",
                "attempt-1",
            );
            paused.fact_id = format!("pause-revision-{index}");
            paused.transition = MetricsTransition::Paused {
                transition_id: format!("pause-revision-{index}"),
            };
            store
                .collect(
                    paused,
                    format!("2026-08-20T00:00:0{}.000", index * 2 - 1),
                    "test",
                    (index * 2 - 1) as i64,
                )
                .unwrap();

            let mut resumed = workflow_fact(
                LifecycleEventType::ExecutionResumed,
                "run-001",
                "round-001",
                "attempt-1",
            );
            resumed.fact_id = format!("continue-action-{index}");
            resumed.transition = MetricsTransition::Resumed {
                transition_id: format!("continue-action-{index}"),
                action: UserExecutionAction::ManualContinue,
                follow_up_action_id: has_follow_up.then(|| format!("continue-action-{index}")),
            };
            store
                .collect(
                    resumed,
                    format!("2026-08-20T00:00:0{}.000", index * 2),
                    "test",
                    (index * 2) as i64,
                )
                .unwrap();
        }

        let terminal = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionCompleted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:05.000".to_string(),
                "test",
                5,
            )
            .unwrap();
        let counters = terminal.counters.unwrap();
        assert_eq!(counters.pause_count, 2);
        assert_eq!(counters.resume_count, 2);
        assert_eq!(counters.manual_continue_count, 2);
        assert_eq!(counters.follow_up_count, 1);
    }

    #[test]
    fn failed_outbox_insert_rolls_back_revision_and_state() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let event_id = "same-event".to_string();
        store
            .collect_with_event_id(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01Z".to_string(),
                "test",
                1,
                event_id.clone(),
            )
            .unwrap();
        let result = store.collect_with_event_id(
            workflow_fact(
                LifecycleEventType::ExecutionStarted,
                "run-002",
                "round-001",
                "attempt-2",
            ),
            "2026-08-20T00:00:02Z".to_string(),
            "test",
            2,
            event_id,
        );
        assert!(result.is_err());
        assert_eq!(store.task_revision("project-1", "task-uuid").unwrap(), 1);
    }

    #[test]
    fn claim_disposition_and_expired_lease_are_recoverable() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let event = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01Z".to_string(),
                "test",
                1,
            )
            .unwrap();
        let first = store.claim_batch("owner-a", 2, 10, 100).unwrap();
        assert_eq!(first.items.len(), 1);
        assert!(
            store
                .claim_batch("owner-b", 3, 10, 100)
                .unwrap()
                .items
                .is_empty()
        );
        let recovered = store.claim_batch("owner-b", 12, 10, 100).unwrap();
        assert_eq!(recovered.items.len(), 1);
        store
            .apply_disposition(
                "owner-b",
                BatchDisposition {
                    accepted_event_ids: vec![event.event_id],
                    duplicate_event_ids: Vec::new(),
                    rejected: Vec::new(),
                },
                13,
            )
            .unwrap();
        assert!(
            store
                .claim_batch("owner-c", 14, 10, 100)
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn retryable_upload_failure_drops_event_after_third_claim() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let event = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01.000".to_string(),
                "test",
                1,
            )
            .unwrap();

        for attempt_count in 1..=MAX_UPLOAD_ATTEMPTS {
            let now = i64::from(attempt_count) * 10;
            let claimed = store.claim_batch("owner", now, 30, 100).unwrap();
            assert_eq!(claimed.discarded_exhausted_count, 0);
            assert_eq!(claimed.items.len(), 1);
            assert_eq!(claimed.items[0].attempt_count, attempt_count);
            let disposition = store
                .record_upload_failure(
                    "owner",
                    &[UploadFailureItem {
                        event_id: event.event_id.clone(),
                        attempt_count,
                        next_attempt_at: now + 1,
                    }],
                    "METRICS_NETWORK_FAILED",
                    true,
                )
                .unwrap();
            if attempt_count < MAX_UPLOAD_ATTEMPTS {
                assert_eq!(
                    disposition,
                    UploadFailureDisposition {
                        rescheduled_count: 1,
                        dropped_count: 0,
                    }
                );
            } else {
                assert_eq!(
                    disposition,
                    UploadFailureDisposition {
                        rescheduled_count: 0,
                        dropped_count: 1,
                    }
                );
            }
        }

        assert!(
            store
                .claim_batch("owner", 100, 30, 100)
                .unwrap()
                .items
                .is_empty()
        );
        let outbox_count: u64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM metrics_outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outbox_count, 0);
    }

    #[test]
    fn non_retryable_failure_drops_event_on_first_claim() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let event = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01.000".to_string(),
                "test",
                1,
            )
            .unwrap();
        let claimed = store.claim_batch("owner", 2, 30, 100).unwrap();

        let disposition = store
            .record_upload_failure(
                "owner",
                &[UploadFailureItem {
                    event_id: event.event_id,
                    attempt_count: claimed.items[0].attempt_count,
                    next_attempt_at: 3,
                }],
                "METRICS_HTTP_401",
                false,
            )
            .unwrap();

        assert_eq!(disposition.rescheduled_count, 0);
        assert_eq!(disposition.dropped_count, 1);
        assert!(
            store
                .claim_batch("owner", 4, 30, 100)
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn claim_discards_legacy_exhausted_pending_without_resending() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let event = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01.000".to_string(),
                "test",
                1,
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE metrics_outbox SET attempt_count = 100 WHERE event_id = ?1",
                params![event.event_id],
            )
            .unwrap();

        let claimed = store.claim_batch("owner", 2, 30, 100).unwrap();

        assert!(claimed.items.is_empty());
        assert_eq!(claimed.discarded_exhausted_count, 1);
    }

    #[test]
    fn mixed_attempt_batch_records_failure_per_event() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let first = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionStarted,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:01.000".to_string(),
                "test",
                1,
            )
            .unwrap();
        let second = store
            .collect(
                workflow_fact(
                    LifecycleEventType::ExecutionPaused,
                    "run-001",
                    "round-001",
                    "attempt-1",
                ),
                "2026-08-20T00:00:02.000".to_string(),
                "test",
                2,
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE metrics_outbox SET attempt_count = 2 WHERE event_id = ?1",
                params![second.event_id],
            )
            .unwrap();

        let claimed = store.claim_batch("owner", 3, 30, 100).unwrap();
        assert_eq!(claimed.items.len(), 2);
        let failure_items = claimed
            .items
            .iter()
            .map(|item| UploadFailureItem {
                event_id: item.event_id.clone(),
                attempt_count: item.attempt_count,
                next_attempt_at: 4,
            })
            .collect::<Vec<_>>();
        let disposition = store
            .record_upload_failure("owner", &failure_items, "METRICS_HTTP_500", true)
            .unwrap();

        assert_eq!(
            disposition,
            UploadFailureDisposition {
                rescheduled_count: 1,
                dropped_count: 1,
            }
        );
        let remaining = store.claim_batch("owner", 4, 30, 100).unwrap();
        assert_eq!(remaining.items.len(), 1);
        assert_eq!(remaining.items[0].event_id, first.event_id);
        assert_eq!(remaining.items[0].attempt_count, 2);
    }

    #[test]
    fn upload_attempt_count_survives_store_restart() {
        let temp = tempdir().unwrap();
        let database_path = temp.path().join("metrics.sqlite3");
        let event_id = {
            let mut store = MetricsCollectorStore::open(&database_path).unwrap();
            let event = store
                .collect(
                    workflow_fact(
                        LifecycleEventType::ExecutionStarted,
                        "run-001",
                        "round-001",
                        "attempt-1",
                    ),
                    "2026-08-20T00:00:01.000".to_string(),
                    "test",
                    1,
                )
                .unwrap();
            let claimed = store.claim_batch("owner-a", 2, 30, 100).unwrap();
            assert_eq!(claimed.items[0].attempt_count, 1);
            store
                .record_upload_failure(
                    "owner-a",
                    &[UploadFailureItem {
                        event_id: event.event_id.clone(),
                        attempt_count: 1,
                        next_attempt_at: 3,
                    }],
                    "METRICS_NETWORK_FAILED",
                    true,
                )
                .unwrap();
            event.event_id
        };

        let mut reopened = MetricsCollectorStore::open(&database_path).unwrap();
        let claimed = reopened.claim_batch("owner-b", 3, 30, 100).unwrap();
        assert_eq!(claimed.items.len(), 1);
        assert_eq!(claimed.items[0].event_id, event_id);
        assert_eq!(claimed.items[0].attempt_count, 2);
    }

    #[test]
    fn delivery_uses_terminal_git_baseline_snapshot_without_accumulating_attempt_churn() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let mut delivery = PendingMetricsFact::new(
            TaskMetricsKey {
                project_id: "project-1".to_string(),
                execution_id: "task-uuid".to_string(),
            },
            LifecycleEventType::ExecutionCompleted,
            "2026-08-20T00:00:03Z".to_string(),
            "user".to_string(),
            "D:/repo".to_string(),
            MetricsSessionMode::Workflow,
            MetricsSubject::WorkflowRun,
            MetricsRuntimeLocator {
                run_id: "run-001".to_string(),
                round_id: "round-002".to_string(),
            },
            MetricsTaskOrigin::User,
            None,
        );
        delivery.payload.outcome = Some(gold_band::app::observability::ExecutionOutcome::Success);
        delivery.payload.terminal_reason = Some(TerminalReason::Completed);
        delivery.payload.code_changes = Some(TaskCodeChanges {
            added_lines: 17,
            deleted_lines: 3,
            changed_files: 2,
        });
        let changes = store
            .collect(delivery, "2026-08-20T00:00:01Z".to_string(), "test", 1)
            .unwrap()
            .code_changes
            .unwrap();
        assert_eq!(changes.added_lines, 17);
        assert_eq!(changes.deleted_lines, 3);
        assert_eq!(changes.changed_files, 2);
    }

    #[test]
    fn claim_batch_never_mixes_report_months() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        for (reported_at, round_id, attempt_id, now) in [
            ("2026-08-31T23:59:59.000", "round-001", "attempt-1", 1),
            ("2026-09-01T00:00:00.000", "round-002", "attempt-2", 2),
        ] {
            store
                .collect(
                    workflow_fact(
                        LifecycleEventType::ExecutionStarted,
                        "run-001",
                        round_id,
                        attempt_id,
                    ),
                    reported_at.to_string(),
                    "test",
                    now,
                )
                .unwrap();
        }
        let august = store.claim_batch("owner-a", 3, 30, 100).unwrap();
        assert_eq!(august.items.len(), 1);
        store
            .apply_disposition(
                "owner-a",
                BatchDisposition {
                    accepted_event_ids: vec![august.items[0].event_id.clone()],
                    duplicate_event_ids: Vec::new(),
                    rejected: Vec::new(),
                },
                4,
            )
            .unwrap();
        assert_eq!(
            store
                .claim_batch("owner-b", 5, 30, 100)
                .unwrap()
                .items
                .len(),
            1
        );
    }
}
