use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use gold_band::app::observability::{
    CodeChangeCompleteness, ExecutionKind, LifecycleEventType, MetricsCounters,
    MetricsExecutionTrigger, MetricsInterventionKind, MetricsPauseReason, MetricsSessionMode,
    MetricsSubject, MetricsTaskOrigin, MetricsTransition, ModelUsage, PendingMetricsFact,
    TaskCodeChangeDelta, TerminalReason, TokenUsage, UserExecutionAction,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskCodeChanges {
    pub added_lines: Option<u64>,
    pub deleted_lines: Option<u64>,
    pub changed_files: Option<u64>,
    pub completeness: CodeChangeCompleteness,
    pub limitation_codes: Vec<String>,
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
    pub execution_trigger: MetricsExecutionTrigger,
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
struct TaskCodeChangeAccumulator {
    added_lines: u64,
    deleted_lines: u64,
    changed_paths: BTreeSet<String>,
    limitation_codes: BTreeSet<String>,
    observed: bool,
    incomplete: bool,
    unavailable: bool,
}

impl TaskCodeChangeAccumulator {
    fn apply(&mut self, delta: TaskCodeChangeDelta) {
        self.observed = true;
        match delta.completeness {
            CodeChangeCompleteness::Complete => {}
            CodeChangeCompleteness::Partial => self.incomplete = true,
            CodeChangeCompleteness::Unavailable => self.unavailable = true,
        }
        for file in delta.files {
            self.added_lines = self.added_lines.saturating_add(file.added_lines);
            self.deleted_lines = self.deleted_lines.saturating_add(file.deleted_lines);
            self.changed_paths.insert(file.logical_path);
        }
        self.limitation_codes.extend(delta.limitation_codes);
    }

    fn snapshot(&self) -> TaskCodeChanges {
        if !self.observed || (self.unavailable && self.changed_paths.is_empty()) {
            return TaskCodeChanges {
                added_lines: None,
                deleted_lines: None,
                changed_files: None,
                completeness: CodeChangeCompleteness::Unavailable,
                limitation_codes: self.limitation_codes.iter().cloned().collect(),
            };
        }
        TaskCodeChanges {
            added_lines: Some(self.added_lines),
            deleted_lines: Some(self.deleted_lines),
            changed_files: Some(self.changed_paths.len() as u64),
            completeness: if self.incomplete || self.unavailable {
                CodeChangeCompleteness::Partial
            } else {
                CodeChangeCompleteness::Complete
            },
            limitation_codes: self.limitation_codes.iter().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TaskMetricsState {
    counters: MetricsCounters,
    code_changes: TaskCodeChangeAccumulator,
    acceptance_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct OutboxItem {
    pub event_id: String,
    pub payload_json: String,
    pub attempt_count: u32,
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
        reply: oneshot::Sender<Result<Vec<OutboxItem>>>,
    },
    ApplyDisposition {
        owner: String,
        disposition: BatchDisposition,
        now_epoch_seconds: i64,
        reply: oneshot::Sender<Result<()>>,
    },
    Retry {
        owner: String,
        event_ids: Vec<String>,
        next_attempt_at: i64,
        error_code: String,
        reply: oneshot::Sender<Result<()>>,
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
            CollectorCommand::Retry {
                owner,
                event_ids,
                next_attempt_at,
                error_code,
                reply,
            } => {
                let _ = reply.send(store.retry_claimed(
                    &owner,
                    &event_ids,
                    next_attempt_at,
                    &error_code,
                ));
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
        if let Some(delta) = fact.payload.code_change_delta.take() {
            task_state.code_changes.apply(delta);
        }

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
        let code_changes =
            (terminal && fact.subject.is_delivery()).then(|| task_state.code_changes.snapshot());
        let event = build_collected_event(
            fact,
            event_id,
            revision,
            reported_at,
            client_version,
            counters,
            code_changes,
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
    ) -> Result<Vec<OutboxItem>> {
        ensure!(!owner.trim().is_empty(), "lease owner is required");
        let limit = limit.clamp(1, 100) as i64;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE metrics_outbox
             SET status = ?1, lease_owner = NULL, lease_until = NULL
             WHERE status = ?2 AND lease_until <= ?3",
            params![OUTBOX_PENDING, OUTBOX_IN_FLIGHT, now_epoch_seconds],
        )?;
        let report_month = transaction
            .query_row(
                "SELECT substr(reported_at, 1, 7) FROM metrics_outbox
                 WHERE status = ?1 AND next_attempt_at <= ?2
                 ORDER BY created_at, event_revision LIMIT 1",
                params![OUTBOX_PENDING, now_epoch_seconds],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(report_month) = report_month else {
            transaction.commit()?;
            return Ok(Vec::new());
        };
        let ids = {
            let mut statement = transaction.prepare(
                "SELECT event_id FROM metrics_outbox
                 WHERE status = ?1 AND next_attempt_at <= ?2
                   AND substr(reported_at, 1, 7) = ?3
                 ORDER BY created_at, event_revision
                 LIMIT ?4",
            )?;
            statement
                .query_map(
                    params![OUTBOX_PENDING, now_epoch_seconds, report_month, limit],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        if ids.is_empty() {
            transaction.commit()?;
            return Ok(Vec::new());
        }
        let lease_until = now_epoch_seconds.saturating_add(lease_seconds.max(1));
        for id in &ids {
            transaction.execute(
                "UPDATE metrics_outbox
                 SET status = ?1, lease_owner = ?2, lease_until = ?3, attempt_count = attempt_count + 1
                 WHERE event_id = ?4 AND status = ?5",
                params![OUTBOX_IN_FLIGHT, owner, lease_until, id, OUTBOX_PENDING],
            )?;
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
        Ok(items)
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

    pub fn retry_claimed(
        &mut self,
        owner: &str,
        event_ids: &[String],
        next_attempt_at: i64,
        error_code: &str,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for id in event_ids {
            transaction.execute(
                "UPDATE metrics_outbox
                 SET status = ?1, next_attempt_at = ?2, lease_owner = NULL, lease_until = NULL,
                     last_error_code = ?3
                 WHERE event_id = ?4 AND status = ?5 AND lease_owner = ?6",
                params![
                    OUTBOX_PENDING,
                    next_attempt_at,
                    error_code,
                    id,
                    OUTBOX_IN_FLIGHT,
                    owner
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
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
    code_changes: Option<TaskCodeChanges>,
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
        code_changes,
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
            MetricsExecutionTrigger::User,
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
            MetricsExecutionTrigger::User,
        );
        delivery.payload.outcome = Some(gold_band::app::observability::ExecutionOutcome::Success);
        delivery.payload.terminal_reason = Some(TerminalReason::Completed);
        let delivery = store
            .collect(delivery, "2026-08-20T00:00:06Z".to_string(), "test", 6)
            .unwrap();
        assert_eq!(delivery.counters.unwrap().pause_count, 1);
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
        assert_eq!(first.len(), 1);
        assert!(store.claim_batch("owner-b", 3, 10, 100).unwrap().is_empty());
        let recovered = store.claim_batch("owner-b", 12, 10, 100).unwrap();
        assert_eq!(recovered.len(), 1);
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
                .is_empty()
        );
    }

    #[test]
    fn code_change_churn_accumulates_across_attempts_and_deduplicates_paths() {
        let temp = tempdir().unwrap();
        let mut store = MetricsCollectorStore::open(&temp.path().join("metrics.sqlite3")).unwrap();
        let delta = |files| TaskCodeChangeDelta {
            completeness: CodeChangeCompleteness::Complete,
            files,
            limitation_codes: Vec::new(),
        };
        let file = |path: &str, added_lines, deleted_lines| {
            gold_band::app::observability::CodeChangeFileDelta {
                logical_path: path.to_string(),
                added_lines,
                deleted_lines,
            }
        };
        let mut first = workflow_fact(
            LifecycleEventType::ExecutionCompleted,
            "run-001",
            "round-001",
            "attempt-1",
        );
        first.payload.code_change_delta = Some(delta(vec![file("src/lib.rs", 10, 2)]));
        store
            .collect(first, "2026-08-20T00:00:01Z".to_string(), "test", 1)
            .unwrap();
        let mut second = workflow_fact(
            LifecycleEventType::ExecutionCompleted,
            "run-001",
            "round-002",
            "attempt-2",
        );
        second.payload.code_change_delta = Some(TaskCodeChangeDelta {
            completeness: CodeChangeCompleteness::Partial,
            files: vec![file("src/lib.rs", 3, 1), file("src/main.rs", 4, 0)],
            limitation_codes: vec!["NON_LINEAR_MUTATION".to_string()],
        });
        store
            .collect(second, "2026-08-20T00:00:02Z".to_string(), "test", 2)
            .unwrap();

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
            MetricsExecutionTrigger::User,
        );
        delivery.payload.outcome = Some(gold_band::app::observability::ExecutionOutcome::Success);
        delivery.payload.terminal_reason = Some(TerminalReason::Completed);
        let changes = store
            .collect(delivery, "2026-08-20T00:00:04Z".to_string(), "test", 4)
            .unwrap()
            .code_changes
            .unwrap();
        assert_eq!(changes.added_lines, Some(17));
        assert_eq!(changes.deleted_lines, Some(3));
        assert_eq!(changes.changed_files, Some(2));
        assert_eq!(changes.completeness, CodeChangeCompleteness::Partial);
        assert_eq!(changes.limitation_codes, vec!["NON_LINEAR_MUTATION"]);
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
        assert_eq!(august.len(), 1);
        store
            .apply_disposition(
                "owner-a",
                BatchDisposition {
                    accepted_event_ids: vec![august[0].event_id.clone()],
                    duplicate_event_ids: Vec::new(),
                    rejected: Vec::new(),
                },
                4,
            )
            .unwrap();
        assert_eq!(store.claim_batch("owner-b", 5, 30, 100).unwrap().len(), 1);
    }
}
