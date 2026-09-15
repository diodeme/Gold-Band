use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{Local, NaiveDate, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;
use walkdir::WalkDir;

use crate::acp::events::{AcpTimelineItem, AcpTimelinePatch};

use super::{
    DirectReplyFact, NodeFact, PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION,
    PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS, PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS,
    PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS, PersonalAnalyticsNarrative, PersonalAnalyticsReport,
    PersonalAnalyticsSemanticItem, ProjectionAccumulator, RunFact, TaskFact, TokenCounters,
    attempt_key, canonicalize_personal_analytics_report, child_run_key, finalize_projection,
    is_current_attempt_usage, is_excluded, is_personal_analytics_source_name, node_group_key,
    node_id, node_provider, normalized_tool_name, parse_prompt_usage, read_bounded_utf8_prefix,
    run_key, safe_relative_path, should_descend_analytics_entry, string_at, task_key,
    timestamp_epoch, u64_at_optional,
};

const ANALYTICS_INDEX_SCHEMA_VERSION: i64 = 9;
const ANALYTICS_INSIGHT_CACHE_RETAINED: i64 = 64;
#[cfg(test)]
const ANALYTICS_PHYSICAL_TABLES: [&str; 8] = [
    "analytics_sources",
    "analytics_index_state",
    "analytics_tasks",
    "analytics_runs",
    "analytics_attempts",
    "analytics_counters",
    "analytics_semantic_samples",
    "analytics_insight_cache",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsDateRange {
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsIndexState {
    pub index_revision: u64,
    pub schema_version: i64,
    pub sync_status: String,
    pub synced_at: Option<String>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsIndexStats {
    pub index_revision: u64,
    pub changed_files: u64,
    pub reparsed_files: u64,
    pub deleted_files: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct SourceFile {
    relative: String,
    path: Utf8PathBuf,
    file_type: &'static str,
    eligible: bool,
    bytes: u64,
    fingerprint: String,
    modified_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct SourceFacts {
    status: &'static str,
    error_code: Option<String>,
    task_update: Option<TaskUpdate>,
    runs: Vec<IndexedRun>,
    attempts: Vec<IndexedAttempt>,
    counters: Vec<IndexedCounter>,
    semantic: Option<IndexedSemantic>,
}

#[derive(Debug, Clone, PartialEq)]
struct TaskUpdate {
    task_locator: String,
    title: Option<String>,
    mode: Option<String>,
    last_activity_epoch: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedRun {
    run_locator: String,
    task_locator: String,
    unit_type: String,
    status: String,
    outcome: Option<String>,
    activity_epoch: i64,
    terminal_node: Option<String>,
    pause_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedAttempt {
    attempt_locator: String,
    run_locator: String,
    task_locator: String,
    node_id: String,
    agent: Option<String>,
    outcome: Option<String>,
    child_run_locator: Option<String>,
    session_elapsed_seconds: Option<u64>,
    activity_epoch: i64,
    token_usage: TokenCounters,
    observed_prompt_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptSourceRole {
    Node,
    Snapshot,
    Usage,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedCounter {
    owner_locator: String,
    owner_type: &'static str,
    activity_epoch: i64,
    kind: &'static str,
    name: String,
    count: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedSemantic {
    activity_epoch: i64,
    item: PersonalAnalyticsSemanticItem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InsightIdentity {
    pub operation_id: String,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
    pub schema_version: String,
    pub index_revision: u64,
    pub agent_type: String,
    pub model_id: Option<String>,
    pub thought_level_option_id: Option<String>,
    pub thought_level_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeBounds {
    All,
    Bounded(i64, i64),
}

impl RangeBounds {
    fn start(self) -> i64 {
        match self {
            Self::All => i64::MIN,
            Self::Bounded(start, _) => start,
        }
    }

    fn end(self) -> i64 {
        match self {
            Self::All => i64::MAX,
            Self::Bounded(_, end) => end,
        }
    }
}

pub struct PersonalAnalyticsIndex {
    conn: Connection,
}

impl PersonalAnalyticsIndex {
    pub fn open(db_path: &Utf8Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path.as_std_path())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )?;
        let index = Self { conn };
        index.ensure_schema()?;
        Ok(index)
    }

    fn ensure_schema(&self) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let state_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='analytics_index_state')",
            [], |row| row.get(0),
        )?;
        let version = if state_exists {
            tx.query_row(
                "SELECT schemaVersion FROM analytics_index_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
        } else {
            None
        };
        if version.is_some() && version != Some(ANALYTICS_INDEX_SCHEMA_VERSION) {
            drop_analytics_schema(&tx)?;
        }
        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS analytics_sources (
                sourcePath TEXT PRIMARY KEY,
                sourceType TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                parseStatus TEXT NOT NULL CHECK (parseStatus IN ('parsed','skipped','corrupt','unknown-version')),
                errorCode TEXT,
                sizeBytes INTEGER NOT NULL,
                activityEpoch INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS analytics_sources_status_idx ON analytics_sources(parseStatus);
            CREATE TABLE IF NOT EXISTS analytics_index_state (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                schemaVersion INTEGER NOT NULL,
                indexRevision INTEGER NOT NULL,
                syncStatus TEXT NOT NULL,
                syncedAt TEXT,
                lastErrorCode TEXT
            );
            CREATE TABLE IF NOT EXISTS analytics_tasks (
                taskLocator TEXT PRIMARY KEY,
                projectLocator TEXT NOT NULL,
                projectName TEXT NOT NULL,
                title TEXT NOT NULL,
                mode TEXT NOT NULL,
                taskSourcePath TEXT,
                conversationSourcePath TEXT,
                lastActivityEpoch INTEGER
            );
            CREATE INDEX IF NOT EXISTS analytics_tasks_activity_idx ON analytics_tasks(lastActivityEpoch);
            CREATE TABLE IF NOT EXISTS analytics_runs (
                runLocator TEXT PRIMARY KEY,
                taskLocator TEXT NOT NULL,
                unitType TEXT NOT NULL CHECK (unitType IN ('workflow-run','auto-outer-run','auto-child-run','direct-session')),
                status TEXT NOT NULL,
                outcome TEXT,
                activityEpoch INTEGER NOT NULL,
                terminalNode TEXT,
                pauseReason TEXT,
                sourcePath TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS analytics_runs_range_idx ON analytics_runs(activityEpoch, taskLocator, unitType);
            CREATE TABLE IF NOT EXISTS analytics_attempts (
                attemptLocator TEXT PRIMARY KEY,
                runLocator TEXT NOT NULL,
                taskLocator TEXT NOT NULL,
                nodeSourcePath TEXT,
                snapshotSourcePath TEXT,
                usageSourcePath TEXT,
                nodeId TEXT NOT NULL,
                agent TEXT,
                outcome TEXT,
                childRunLocator TEXT,
                sessionElapsedSeconds INTEGER,
                zeroFilled INTEGER NOT NULL,
                activityEpoch INTEGER NOT NULL,
                observedPromptCount INTEGER NOT NULL DEFAULT 0,
                inputTokens INTEGER NOT NULL,
                outputTokens INTEGER NOT NULL,
                cacheReadTokens INTEGER NOT NULL,
                cacheWriteTokens INTEGER NOT NULL,
                totalTokens INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS analytics_attempts_range_idx ON analytics_attempts(activityEpoch, runLocator, taskLocator);
            CREATE INDEX IF NOT EXISTS analytics_attempts_child_run_idx ON analytics_attempts(taskLocator, childRunLocator)
                WHERE childRunLocator IS NOT NULL;
            CREATE TABLE IF NOT EXISTS analytics_counters (
                sourcePath TEXT NOT NULL,
                ownerType TEXT NOT NULL CHECK (ownerType IN ('run','attempt','task')),
                ownerLocator TEXT NOT NULL,
                activityEpoch INTEGER NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('tool','permission','elicitation','pause','resume','manual-continue','skill','event','agent','prompt-status')),
                name TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (sourcePath, ownerType, ownerLocator, activityEpoch, kind, name)
            );
            CREATE INDEX IF NOT EXISTS analytics_counters_range_idx ON analytics_counters(activityEpoch, kind, name);
            CREATE TABLE IF NOT EXISTS analytics_semantic_samples (
                sourcePath TEXT PRIMARY KEY,
                locator TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                activityEpoch INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS analytics_insight_cache (
                sourceOperationId TEXT PRIMARY KEY,
                rangeStart TEXT,
                rangeEnd TEXT,
                schemaVersion TEXT NOT NULL,
                indexRevision INTEGER NOT NULL,
                agentType TEXT NOT NULL,
                modelId TEXT,
                thoughtLevelOptionId TEXT,
                thoughtLevelValue TEXT,
                insightsJson TEXT NOT NULL,
                completedAt TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS analytics_insight_cache_identity_idx
                ON analytics_insight_cache(
                    COALESCE(rangeStart, ''), COALESCE(rangeEnd, ''),
                    schemaVersion, indexRevision, agentType, COALESCE(modelId, ''),
                    COALESCE(thoughtLevelOptionId, ''), COALESCE(thoughtLevelValue, '')
                );
            CREATE VIEW IF NOT EXISTS analytics_projects AS
                SELECT projectLocator, projectName, COUNT(DISTINCT taskLocator) AS taskCount,
                       MIN(lastActivityEpoch) AS firstActivityEpoch, MAX(lastActivityEpoch) AS lastActivityEpoch
                FROM analytics_tasks GROUP BY projectLocator, projectName;
            CREATE VIEW IF NOT EXISTS analytics_usage AS
                SELECT taskLocator, SUM(inputTokens) AS inputTokens, SUM(outputTokens) AS outputTokens,
                       SUM(cacheReadTokens) AS cacheReadTokens, SUM(cacheWriteTokens) AS cacheWriteTokens,
                       SUM(totalTokens) AS totalTokens,
                       COUNT(*) FILTER (WHERE nodeSourcePath IS NOT NULL OR snapshotSourcePath IS NOT NULL) AS attemptCount,
                       SUM(observedPromptCount) AS observedPromptCount
                FROM analytics_attempts GROUP BY taskLocator;
            CREATE VIEW IF NOT EXISTS analytics_event_counts AS
                SELECT kind, name, SUM(count) AS count FROM analytics_counters
                WHERE kind != 'prompt-status' GROUP BY kind, name;
            CREATE VIEW IF NOT EXISTS analytics_insights AS
                SELECT sourceOperationId, value->>'$.section' AS section, value->>'$.title' AS title
                FROM analytics_insight_cache, json_each(insightsJson, '$.insights');
            "#,
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO analytics_index_state(singleton, schemaVersion, indexRevision, syncStatus)
             VALUES (1, ?1, 0, 'idle')",
            [ANALYTICS_INDEX_SCHEMA_VERSION],
        )?;
        tx.commit()
    }

    pub fn state(&self) -> rusqlite::Result<AnalyticsIndexState> {
        self.conn.query_row(
            "SELECT indexRevision, schemaVersion, syncStatus, syncedAt, lastErrorCode
             FROM analytics_index_state WHERE singleton = 1",
            [],
            |row| {
                Ok(AnalyticsIndexState {
                    index_revision: row.get(0)?,
                    schema_version: row.get(1)?,
                    sync_status: row.get(2)?,
                    synced_at: row.get(3)?,
                    last_error_code: row.get(4)?,
                })
            },
        )
    }

    pub fn sync<F, C>(
        &mut self,
        projects_root: &Utf8Path,
        progress: F,
        cancelled: C,
    ) -> Result<AnalyticsIndexStats>
    where
        F: FnMut(u64, u64) + Send,
        C: Fn() -> bool + Sync,
    {
        let started = std::time::Instant::now();
        let canonical_root = projects_root.canonicalize_utf8()?;
        let discovery_started = std::time::Instant::now();
        let files = discover_sources(&canonical_root)?;
        let existing = self.existing_sources()?;
        let discovery_ms = discovery_started.elapsed().as_millis() as u64;
        let total = files.len() as u64;
        let mut pending = Vec::new();
        let mut reparsed_files = 0u64;
        let progress = Mutex::new(progress);
        let comparison_started = std::time::Instant::now();
        for (index, source) in files.iter().enumerate() {
            if cancelled() {
                bail!("analytics.cancelled");
            }
            if index % 32 == 0 || index + 1 == files.len() {
                (progress
                    .lock()
                    .map_err(|_| anyhow::anyhow!("analytics.state-unavailable"))?)(
                    index as u64,
                    total,
                );
            }
            if existing
                .get(&source.relative)
                .is_some_and(|fingerprint| *fingerprint == source.fingerprint)
            {
                continue;
            }
            reparsed_files += u64::from(source.eligible);
            pending.push(source);
        }
        let comparison_ms = comparison_started.elapsed().as_millis() as u64;
        let parse_started = std::time::Instant::now();
        let replacements = Self::parse_sources_parallel(pending, total, &progress, &cancelled)?;
        let parse_ms = parse_started.elapsed().as_millis() as u64;
        let current_paths = files
            .iter()
            .map(|file| file.relative.as_str())
            .collect::<BTreeSet<_>>();
        let deleted_files = existing
            .keys()
            .filter(|path| !current_paths.contains(path.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        (progress
            .lock()
            .map_err(|_| anyhow::anyhow!("analytics.state-unavailable"))?)(total, total);

        if cancelled() {
            bail!("analytics.cancelled");
        }
        let write_started = std::time::Instant::now();
        let tx = self.conn.transaction()?;
        mark_sync(&tx, "syncing", None)?;
        let mut run_type_tasks = BTreeSet::new();
        for path in &deleted_files {
            run_type_tasks.extend(delete_source_facts(&tx, path)?);
            tx.execute(
                "DELETE FROM analytics_sources WHERE sourcePath = ?1",
                [path],
            )?;
        }
        for (source, facts) in &replacements {
            run_type_tasks.extend(upsert_source(&tx, source, facts)?);
        }
        cleanup_orphaned_analytics_facts(&tx)?;
        for task_locator in run_type_tasks {
            refresh_run_types(&tx, &task_locator)?;
        }
        let index_revision = if replacements.is_empty() && deleted_files.is_empty() {
            current_revision(&tx)?
        } else {
            next_revision(&tx)?
        };
        tx.execute(
            "UPDATE analytics_index_state SET syncedAt = ?1, lastErrorCode = NULL WHERE singleton = 1",
            [Utc::now().to_rfc3339()],
        )?;
        mark_sync(&tx, "idle", None)?;
        tx.commit()?;
        let write_ms = write_started.elapsed().as_millis() as u64;
        let duration_ms = started.elapsed().as_millis() as u64;
        info!(
            target: "gold_band::perf",
            operation = "personal_analytics_sync",
            discovered_sources = total,
            replacement_sources = replacements.len(),
            deleted_sources = deleted_files.len(),
            discovery_ms,
            comparison_ms,
            parse_ms,
            write_ms,
            duration_ms,
            "personal analytics index sync completed"
        );
        Ok(AnalyticsIndexStats {
            index_revision,
            changed_files: reparsed_files + deleted_files.len() as u64,
            reparsed_files,
            deleted_files: deleted_files.len() as u64,
            duration_ms,
        })
    }

    fn parse_sources_parallel<'a, F, C>(
        sources: Vec<&'a SourceFile>,
        total: u64,
        progress: &Mutex<F>,
        cancelled: &C,
    ) -> Result<Vec<(&'a SourceFile, SourceFacts)>>
    where
        F: FnMut(u64, u64) + Send,
        C: Fn() -> bool + Sync,
    {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let worker_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(4)
            .min(sources.len())
            .max(1);
        let next_index = AtomicUsize::new(0);
        let processed_count = AtomicUsize::new(0);
        let last_progress = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            let handles = (0..worker_count)
                .map(|_| {
                    scope.spawn(|| {
                        let mut parsed = Vec::new();
                        loop {
                            let index = next_index.fetch_add(1, Ordering::AcqRel);
                            let Some(source) = sources.get(index) else {
                                break;
                            };
                            if cancelled() {
                                bail!("analytics.cancelled");
                            }
                            parsed.push((index, *source, parse_source(source)));
                            let processed = processed_count.fetch_add(1, Ordering::AcqRel) + 1;
                            if processed % 32 == 0 {
                                let previous = last_progress.fetch_max(processed, Ordering::AcqRel);
                                if processed > previous {
                                    (progress.lock().map_err(|_| {
                                        anyhow::anyhow!("analytics.state-unavailable")
                                    })?)(
                                        processed as u64, total
                                    );
                                }
                            }
                        }
                        Ok(parsed)
                    })
                })
                .collect::<Vec<_>>();
            let mut parsed = Vec::new();
            for handle in handles {
                parsed.extend(
                    handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("analytics.index-worker-failed"))??,
                );
            }
            parsed.sort_by_key(|(index, _, _)| *index);
            Ok(parsed
                .into_iter()
                .map(|(_, source, facts)| (source, facts))
                .collect())
        })
    }

    fn existing_sources(&self) -> rusqlite::Result<BTreeMap<String, String>> {
        let mut statement = self
            .conn
            .prepare("SELECT sourcePath, fingerprint FROM analytics_sources")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect()
    }

    pub fn report(
        &self,
        range: &PersonalAnalyticsDateRange,
        report_id: String,
    ) -> Result<PersonalAnalyticsReport> {
        let transaction = self.conn.unchecked_transaction()?;
        let (report, _) = Self::report_in_transaction(&transaction, range, report_id)?;
        transaction.commit()?;
        Ok(report)
    }

    pub fn report_with_semantic_batch(
        &self,
        range: &PersonalAnalyticsDateRange,
        report_id: String,
    ) -> Result<(PersonalAnalyticsReport, Vec<PersonalAnalyticsSemanticItem>)> {
        let transaction = self.conn.unchecked_transaction()?;
        let (report, semantic_items) = Self::report_in_transaction(&transaction, range, report_id)?;
        transaction.commit()?;
        Ok((report, semantic_items))
    }

    fn report_in_transaction(
        transaction: &Transaction<'_>,
        range: &PersonalAnalyticsDateRange,
        report_id: String,
    ) -> Result<(PersonalAnalyticsReport, Vec<PersonalAnalyticsSemanticItem>)> {
        let bounds = range_bounds(range)?;
        let state = transaction
            .query_row(
                "SELECT indexRevision, schemaVersion, syncStatus, syncedAt, lastErrorCode
                 FROM analytics_index_state WHERE singleton = 1",
                [],
                |row| {
                    Ok(AnalyticsIndexState {
                        index_revision: row.get(0)?,
                        schema_version: row.get(1)?,
                        sync_status: row.get(2)?,
                        synced_at: row.get(3)?,
                        last_error_code: row.get(4)?,
                    })
                },
            )
            .map_err(anyhow::Error::from)?;
        let mut accumulator = ProjectionAccumulator::default();
        load_tasks(transaction, bounds, &mut accumulator)?;
        load_runs(transaction, bounds, &mut accumulator)?;
        load_attempts(transaction, bounds, &mut accumulator)?;
        load_counters(transaction, bounds, &mut accumulator)?;
        load_semantic(transaction, bounds, &mut accumulator)?;
        load_coverage(transaction, &mut accumulator)?;
        let output = finalize_projection(accumulator, state.index_revision.to_string());
        let narrative = PersonalAnalyticsNarrative {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            insights: Vec::new(),
        };
        let mut report = canonicalize_personal_analytics_report(
            &output.projection,
            narrative,
            report_id,
            Utc::now().to_rfc3339(),
        );
        report.index_revision = state.index_revision;
        report.range = range.clone();
        Ok((report, output.semantic_batch.items))
    }

    pub fn completed_insight(
        &self,
        identity: &InsightIdentity,
    ) -> rusqlite::Result<Option<PersonalAnalyticsNarrative>> {
        Self::completed_insight_in_transaction(&self.conn, identity)
    }

    fn completed_insight_in_transaction(
        conn: &Connection,
        identity: &InsightIdentity,
    ) -> rusqlite::Result<Option<PersonalAnalyticsNarrative>> {
        let payload = conn
            .query_row(
                "SELECT insightsJson FROM analytics_insight_cache
                 WHERE rangeStart IS ?1 AND rangeEnd IS ?2 AND schemaVersion = ?3
                   AND indexRevision = ?4 AND agentType = ?5 AND modelId IS ?6
                   AND thoughtLevelOptionId IS ?7 AND thoughtLevelValue IS ?8
                 ORDER BY completedAt DESC LIMIT 1",
                rusqlite::params![
                    identity.range_start,
                    identity.range_end,
                    identity.schema_version,
                    identity.index_revision,
                    identity.agent_type,
                    identity.model_id,
                    identity.thought_level_option_id,
                    identity.thought_level_value,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })
            .transpose()
    }

    pub fn store_completed_insight(
        &mut self,
        identity: &InsightIdentity,
        narrative: &PersonalAnalyticsNarrative,
        now: &str,
    ) -> rusqlite::Result<()> {
        let payload = serde_json::to_string(narrative)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT OR REPLACE INTO analytics_insight_cache
             (sourceOperationId, rangeStart, rangeEnd, schemaVersion, indexRevision, agentType,
               modelId, thoughtLevelOptionId, thoughtLevelValue, insightsJson, completedAt)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                identity.operation_id,
                identity.range_start,
                identity.range_end,
                identity.schema_version,
                identity.index_revision,
                identity.agent_type,
                identity.model_id,
                identity.thought_level_option_id,
                identity.thought_level_value,
                payload,
                now
            ],
        )?;
        prune_insight_cache_in_transaction(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn prune_insight_cache_in_transaction(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM analytics_insight_cache
         WHERE sourceOperationId NOT IN (
               SELECT sourceOperationId FROM analytics_insight_cache
               ORDER BY completedAt DESC, sourceOperationId DESC LIMIT ?1
           )",
        [ANALYTICS_INSIGHT_CACHE_RETAINED],
    )?;
    Ok(())
}

fn drop_analytics_schema(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DROP VIEW IF EXISTS analytics_projects;
         DROP VIEW IF EXISTS analytics_usage;
         DROP VIEW IF EXISTS analytics_event_counts;
         DROP VIEW IF EXISTS analytics_insights;
         DROP TABLE IF EXISTS analytics_insight_runs;
         DROP TABLE IF EXISTS analytics_insight_cache;
         DROP TABLE IF EXISTS analytics_semantic_samples;
         DROP TABLE IF EXISTS analytics_counters;
         DROP TABLE IF EXISTS analytics_attempts;
         DROP TABLE IF EXISTS analytics_runs;
         DROP TABLE IF EXISTS analytics_tasks;
         DROP TABLE IF EXISTS analytics_index_state;
         DROP TABLE IF EXISTS analytics_sources;",
    )
}

fn mark_sync(tx: &Transaction<'_>, status: &str, error: Option<&str>) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE analytics_index_state SET syncStatus = ?1, lastErrorCode = ?2 WHERE singleton = 1",
        rusqlite::params![status, error],
    )?;
    Ok(())
}

fn next_revision(tx: &Transaction<'_>) -> rusqlite::Result<u64> {
    let current: i64 = tx.query_row(
        "SELECT indexRevision FROM analytics_index_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let next = current.saturating_add(1);
    tx.execute(
        "UPDATE analytics_index_state SET indexRevision = ?1 WHERE singleton = 1",
        [next],
    )?;
    Ok(next as u64)
}

fn current_revision(tx: &Transaction<'_>) -> rusqlite::Result<u64> {
    Ok(tx.query_row(
        "SELECT indexRevision FROM analytics_index_state WHERE singleton = 1",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

fn upsert_source(
    tx: &Transaction<'_>,
    source: &SourceFile,
    facts: &SourceFacts,
) -> rusqlite::Result<BTreeSet<String>> {
    let mut task_locators = BTreeSet::new();
    execute_cached(
        tx,
        "INSERT OR REPLACE INTO analytics_sources
         (sourcePath, sourceType, fingerprint, parseStatus, errorCode, sizeBytes, activityEpoch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            source.relative,
            source.file_type,
            source.fingerprint,
            facts.status,
            facts.error_code,
            source.bytes,
            source.modified_epoch
        ],
    )?;
    task_locators.extend(delete_source_facts(tx, &source.relative)?);
    if let Some(update) = &facts.task_update {
        let project_locator = update
            .task_locator
            .split('/')
            .next()
            .unwrap_or("project")
            .to_string();
        let fallback = update
            .task_locator
            .rsplit('/')
            .next()
            .unwrap_or("Task")
            .to_string();
        execute_cached(
            tx,
            "INSERT INTO analytics_tasks
             (taskLocator, projectLocator, projectName, title, mode, taskSourcePath,
              conversationSourcePath, lastActivityEpoch)
             VALUES (?1, ?2, ?2, COALESCE(?3, ?4), COALESCE(?5, 'unknown'), ?6, ?7, ?8)
             ON CONFLICT(taskLocator) DO UPDATE SET
               title = CASE WHEN excluded.taskSourcePath IS NOT NULL THEN excluded.title ELSE analytics_tasks.title END,
               mode = CASE WHEN excluded.conversationSourcePath IS NOT NULL THEN excluded.mode ELSE analytics_tasks.mode END,
               taskSourcePath = COALESCE(excluded.taskSourcePath, analytics_tasks.taskSourcePath),
               conversationSourcePath = COALESCE(excluded.conversationSourcePath, analytics_tasks.conversationSourcePath),
               lastActivityEpoch = MAX(COALESCE(excluded.lastActivityEpoch, 0), COALESCE(analytics_tasks.lastActivityEpoch, 0))",
            rusqlite::params![
                update.task_locator,
                project_locator,
                update.title,
                fallback,
                update.mode,
                (source.file_type == "task").then(|| source.relative.clone()),
                (source.file_type == "conversation").then(|| source.relative.clone()),
                update.last_activity_epoch
            ],
        )?;
        task_locators.insert(update.task_locator.clone());
    }
    for run in &facts.runs {
        execute_cached(
            tx,
            "INSERT OR REPLACE INTO analytics_runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                run.run_locator,
                run.task_locator,
                run.unit_type,
                run.status,
                run.outcome,
                run.activity_epoch,
                run.terminal_node,
                run.pause_reason,
                source.relative
            ],
        )?;
        task_locators.insert(run.task_locator.clone());
    }
    for attempt in &facts.attempts {
        let role = match source.file_type {
            "node" => AttemptSourceRole::Node,
            "snapshot" => AttemptSourceRole::Snapshot,
            "usage" => AttemptSourceRole::Usage,
            _ => continue,
        };
        upsert_attempt(tx, source, attempt, role)?;
        task_locators.insert(attempt.task_locator.clone());
    }
    for counter in &facts.counters {
        execute_cached(
            tx,
            "INSERT OR REPLACE INTO analytics_counters VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                source.relative,
                counter.owner_type,
                counter.owner_locator,
                counter.activity_epoch,
                counter.kind,
                counter.name,
                counter.count
            ],
        )?;
    }
    if let Some(semantic) = &facts.semantic {
        execute_cached(
            tx,
            "INSERT OR REPLACE INTO analytics_semantic_samples VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                source.relative,
                semantic.item.locator,
                semantic.item.kind,
                semantic.item.content,
                semantic.activity_epoch
            ],
        )?;
    }
    Ok(task_locators)
}

fn upsert_attempt(
    tx: &Transaction<'_>,
    source: &SourceFile,
    attempt: &IndexedAttempt,
    role: AttemptSourceRole,
) -> rusqlite::Result<()> {
    match role {
        AttemptSourceRole::Node => execute_cached(
            tx,
            "INSERT INTO analytics_attempts
             (attemptLocator, runLocator, taskLocator, nodeSourcePath, snapshotSourcePath,
              usageSourcePath, nodeId, agent, outcome, childRunLocator,
              sessionElapsedSeconds, zeroFilled, activityEpoch,
              inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, totalTokens)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, NULL, 1, ?9, 0, 0, 0, 0, 0)
             ON CONFLICT(attemptLocator) DO UPDATE SET
               runLocator = excluded.runLocator,
               taskLocator = excluded.taskLocator,
               nodeSourcePath = excluded.nodeSourcePath,
               nodeId = excluded.nodeId,
               agent = excluded.agent,
               outcome = excluded.outcome,
               childRunLocator = excluded.childRunLocator,
               activityEpoch = MAX(analytics_attempts.activityEpoch, excluded.activityEpoch)",
            rusqlite::params![
                attempt.attempt_locator,
                attempt.run_locator,
                attempt.task_locator,
                source.relative,
                attempt.node_id,
                attempt.agent,
                attempt.outcome,
                attempt.child_run_locator,
                attempt.activity_epoch
            ],
        ),
        AttemptSourceRole::Snapshot => execute_cached(
            tx,
            "INSERT INTO analytics_attempts
             (attemptLocator, runLocator, taskLocator, nodeSourcePath, snapshotSourcePath,
              usageSourcePath, nodeId, agent, outcome, childRunLocator,
              sessionElapsedSeconds, zeroFilled, activityEpoch,
              inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, totalTokens)
             VALUES (?1, ?2, ?3, NULL, ?4, NULL, 'unknown', NULL, NULL, NULL, ?5, ?6, ?7, 0, 0, 0, 0, 0)
             ON CONFLICT(attemptLocator) DO UPDATE SET
               runLocator = excluded.runLocator,
               taskLocator = excluded.taskLocator,
               snapshotSourcePath = excluded.snapshotSourcePath,
               sessionElapsedSeconds = excluded.sessionElapsedSeconds,
               zeroFilled = excluded.zeroFilled,
               activityEpoch = MAX(analytics_attempts.activityEpoch, excluded.activityEpoch)",
            rusqlite::params![
                attempt.attempt_locator,
                attempt.run_locator,
                attempt.task_locator,
                source.relative,
                attempt.session_elapsed_seconds,
                attempt.session_elapsed_seconds.is_none(),
                attempt.activity_epoch
            ],
        ),
        AttemptSourceRole::Usage => execute_cached(
            tx,
            "INSERT INTO analytics_attempts
             (attemptLocator, runLocator, taskLocator, nodeSourcePath, snapshotSourcePath,
              usageSourcePath, nodeId, agent, outcome, childRunLocator,
              sessionElapsedSeconds, zeroFilled, activityEpoch,
              observedPromptCount, inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, totalTokens)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, 'unknown', NULL, NULL, NULL, NULL, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(attemptLocator) DO UPDATE SET
               runLocator = excluded.runLocator,
               taskLocator = excluded.taskLocator,
               usageSourcePath = excluded.usageSourcePath,
               activityEpoch = MAX(analytics_attempts.activityEpoch, excluded.activityEpoch),
               observedPromptCount = excluded.observedPromptCount,
               inputTokens = excluded.inputTokens,
               outputTokens = excluded.outputTokens,
               cacheReadTokens = excluded.cacheReadTokens,
               cacheWriteTokens = excluded.cacheWriteTokens,
               totalTokens = excluded.totalTokens",
            rusqlite::params![
                attempt.attempt_locator,
                attempt.run_locator,
                attempt.task_locator,
                source.relative,
                attempt.activity_epoch,
                attempt.observed_prompt_count,
                attempt.token_usage.input,
                attempt.token_usage.output,
                attempt.token_usage.cache_read,
                attempt.token_usage.cache_write,
                attempt.token_usage.total
            ],
        ),
    }?;
    Ok(())
}

fn delete_source_facts(
    tx: &Transaction<'_>,
    source_path: &str,
) -> rusqlite::Result<BTreeSet<String>> {
    let mut run_type_tasks = BTreeSet::new();
    if source_path.ends_with("/run.json") {
        execute_cached(
            tx,
            "DELETE FROM analytics_runs WHERE sourcePath = ?1",
            [source_path],
        )?;
    } else if source_path.ends_with("/node.json") {
        let mut statement = tx.prepare(
            "SELECT DISTINCT taskLocator FROM analytics_attempts
             WHERE nodeSourcePath = ?1 AND childRunLocator IS NOT NULL",
        )?;
        run_type_tasks.extend(
            statement
                .query_map([source_path], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
        execute_cached(
            tx,
            "UPDATE analytics_attempts
             SET nodeSourcePath = NULL, nodeId = 'unknown', agent = NULL,
                 outcome = NULL, childRunLocator = NULL
             WHERE nodeSourcePath = ?1",
            [source_path],
        )?;
    } else if source_path.ends_with("/acp.snapshot.json") {
        execute_cached(
            tx,
            "UPDATE analytics_attempts
             SET snapshotSourcePath = NULL, sessionElapsedSeconds = NULL, zeroFilled = 1
             WHERE snapshotSourcePath = ?1",
            [source_path],
        )?;
    } else if source_path.ends_with("/acp.prompt-usage.jsonl") {
        execute_cached(
            tx,
            "UPDATE analytics_attempts
             SET usageSourcePath = NULL, inputTokens = 0, outputTokens = 0,
                 cacheReadTokens = 0, cacheWriteTokens = 0, totalTokens = 0,
                 observedPromptCount = 0
             WHERE usageSourcePath = ?1",
            [source_path],
        )?;
    }
    if source_path.ends_with("/acp.prompt-usage.jsonl")
        || source_path.ends_with("/acp.timeline.jsonl")
        || source_path.ends_with("/observability.snapshot.json")
    {
        execute_cached(
            tx,
            "DELETE FROM analytics_counters WHERE sourcePath = ?1",
            [source_path],
        )?;
    }
    if source_path.ends_with("/requirement.md") {
        execute_cached(
            tx,
            "DELETE FROM analytics_semantic_samples WHERE sourcePath = ?1",
            [source_path],
        )?;
    }
    if source_path.ends_with("/task.json") {
        execute_cached(
            tx,
            "UPDATE analytics_tasks SET title = taskLocator, taskSourcePath = NULL WHERE taskSourcePath = ?1",
            [source_path],
        )?;
    } else if source_path.ends_with("/conversation.json") {
        execute_cached(
            tx,
            "UPDATE analytics_tasks SET mode = 'unknown', conversationSourcePath = NULL,
                    lastActivityEpoch = NULL WHERE conversationSourcePath = ?1",
            [source_path],
        )?;
    }
    Ok(run_type_tasks)
}

fn cleanup_orphaned_analytics_facts(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM analytics_attempts
         WHERE nodeSourcePath IS NULL AND snapshotSourcePath IS NULL AND usageSourcePath IS NULL",
        [],
    )?;
    tx.execute(
        "DELETE FROM analytics_tasks
         WHERE taskSourcePath IS NULL AND conversationSourcePath IS NULL
           AND NOT EXISTS (SELECT 1 FROM analytics_runs WHERE taskLocator = analytics_tasks.taskLocator)
           AND NOT EXISTS (SELECT 1 FROM analytics_attempts WHERE taskLocator = analytics_tasks.taskLocator)",
        [],
    )?;
    Ok(())
}

fn execute_cached<P>(tx: &Transaction<'_>, sql: &str, params: P) -> rusqlite::Result<usize>
where
    P: rusqlite::Params,
{
    tx.prepare_cached(sql)?.execute(params)
}

fn refresh_run_types(tx: &Transaction<'_>, task_locator: &str) -> rusqlite::Result<()> {
    execute_cached(
        tx,
        "UPDATE analytics_runs SET unitType = CASE
           WHEN (SELECT mode FROM analytics_tasks WHERE taskLocator = ?1) = 'direct' THEN 'direct-session'
           WHEN (SELECT mode FROM analytics_tasks WHERE taskLocator = ?1) = 'auto'
             AND EXISTS (
               SELECT 1 FROM analytics_attempts a
               WHERE a.taskLocator = ?1 AND a.childRunLocator = analytics_runs.runLocator
             ) THEN 'auto-child-run'
           WHEN (SELECT mode FROM analytics_tasks WHERE taskLocator = ?1) = 'auto' THEN 'auto-outer-run'
           ELSE 'workflow-run'
         END WHERE taskLocator = ?1",
        [task_locator],
    )?;
    Ok(())
}

fn discover_sources(root: &Utf8Path) -> Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(should_descend_analytics_entry)
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(file_name) = entry.file_name().to_str() else {
            continue;
        };
        if !is_personal_analytics_source_name(file_name) {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
            .map_err(|path| anyhow::anyhow!("invalid UTF-8 analytics path: {}", path.display()))?;
        let Some(relative) = safe_relative_path(root, &path, entry.file_type().is_symlink()) else {
            continue;
        };
        if is_excluded(&relative, file_name) {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified = metadata
            .modified()
            .ok()
            .map(|time| {
                time.duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| (duration.as_secs() as i64, duration.as_nanos() as i64))
                    .unwrap_or((0, 0))
            })
            .unwrap_or((0, 0));
        let file_type = source_type(&relative, file_name);
        files.push(SourceFile {
            path,
            relative,
            file_type,
            eligible: file_type != "other",
            bytes: metadata.len(),
            fingerprint: format!(
                "{}:{}:{}:{}",
                metadata.len(),
                modified.1,
                modified.0,
                file_type
            ),
            modified_epoch: modified.0,
        });
    }
    Ok(files)
}

fn source_type(relative: &str, file_name: &str) -> &'static str {
    match file_name {
        "project.json" => "project",
        "task.json" => "task",
        "conversation.json" => "conversation",
        "run.json" => "run",
        "node.json" => "node",
        "acp.snapshot.json" => "snapshot",
        "observability.snapshot.json" => "observability",
        "acp.prompt-usage.jsonl" if is_current_attempt_usage(relative) => "usage",
        "acp.timeline.jsonl" => "timeline",
        "requirement.md" => "semantic",
        _ => "other",
    }
}

fn parse_source(source: &SourceFile) -> SourceFacts {
    if !source.eligible {
        return SourceFacts {
            status: "skipped",
            ..SourceFacts::default()
        };
    }
    if source.file_type == "semantic" {
        return parse_semantic(source);
    }
    let parsed = if matches!(source.file_type, "usage" | "timeline") {
        parse_jsonl(source)
    } else {
        parse_json(source)
    };
    match parsed {
        Ok(mut facts) => {
            if facts.status == "parsed" && source.file_type == "snapshot" {
                facts.status = "parsed";
            }
            facts
        }
        Err(_) => SourceFacts {
            status: "corrupt",
            error_code: Some("analytics.source-corrupt".to_string()),
            ..SourceFacts::default()
        },
    }
}

fn parse_json(source: &SourceFile) -> Result<SourceFacts> {
    let value: serde_json::Value =
        serde_json::from_reader(BufReader::new(File::open(&source.path)?))?;
    let mut facts = SourceFacts {
        status: "parsed",
        ..SourceFacts::default()
    };
    if !matches!(
        value.get("version").and_then(|version| version.as_str()),
        None | Some("0.1" | "1" | "1.0")
    ) {
        facts.status = "unknown-version";
    }
    let task_locator = task_key(&source.relative);
    match source.file_type {
        "task" => {
            facts.task_update = Some(TaskUpdate {
                task_locator: task_locator.unwrap_or_default(),
                title: value
                    .get("title")
                    .and_then(|title| title.as_str())
                    .map(str::to_string),
                mode: None,
                last_activity_epoch: None,
            });
        }
        "conversation" => {
            facts.task_update = Some(TaskUpdate {
                task_locator: task_locator.clone().unwrap_or_default(),
                title: None,
                mode: value
                    .get("runMode")
                    .and_then(|mode| mode.as_str())
                    .map(str::to_string),
                last_activity_epoch: string_at(&value, &["/lastActivityAt", "/createdAt"])
                    .and_then(timestamp_epoch),
            });
        }
        "run" => {
            let task_locator = task_locator.unwrap_or_default();
            facts.runs.push(IndexedRun {
                run_locator: run_key(&source.relative).unwrap_or_else(|| source.relative.clone()),
                task_locator,
                unit_type: "workflow-run".to_string(),
                status: value
                    .get("status")
                    .and_then(|status| status.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                outcome: value
                    .get("outcome")
                    .and_then(|outcome| outcome.as_str())
                    .map(str::to_string),
                activity_epoch: string_at(&value, &["/updated_at", "/updatedAt"])
                    .and_then(timestamp_epoch)
                    .unwrap_or(source.modified_epoch),
                terminal_node: value
                    .get("last_executed_node")
                    .or_else(|| value.get("current_node"))
                    .and_then(|node| node.as_str())
                    .map(str::to_string),
                pause_reason: value
                    .get("pause_reason")
                    .and_then(|reason| reason.as_str())
                    .map(str::to_string),
            });
        }
        "node" => {
            let task_locator = task_locator.unwrap_or_default();
            facts.attempts.push(IndexedAttempt {
                attempt_locator: attempt_key(&source.relative)
                    .unwrap_or_else(|| source.relative.clone()),
                run_locator: run_key(&source.relative).unwrap_or_default(),
                task_locator,
                node_id: node_id(&source.relative, &value)
                    .unwrap_or("unknown")
                    .to_string(),
                agent: node_provider(&source.relative, &value).map(str::to_string),
                outcome: value
                    .get("outcome")
                    .and_then(|outcome| outcome.as_str())
                    .map(str::to_string),
                child_run_locator: child_run_key(&source.relative, &value),
                session_elapsed_seconds: None,
                activity_epoch: source.modified_epoch,
                token_usage: TokenCounters::default(),
                observed_prompt_count: 0,
            });
        }
        "snapshot" => {
            facts.attempts.push(IndexedAttempt {
                attempt_locator: attempt_key(&source.relative)
                    .unwrap_or_else(|| source.relative.clone()),
                run_locator: run_key(&source.relative).unwrap_or_default(),
                task_locator: task_locator.unwrap_or_default(),
                node_id: "unknown".to_string(),
                agent: None,
                outcome: None,
                child_run_locator: None,
                session_elapsed_seconds: u64_at_optional(
                    &value,
                    &["/timing/sessionElapsedSeconds"],
                ),
                activity_epoch: source.modified_epoch,
                token_usage: TokenCounters::default(),
                observed_prompt_count: 0,
            });
        }
        "observability" => {
            for (kind, pointer) in [
                ("pause", "/counters/pauseCount"),
                ("resume", "/counters/resumeCount"),
                ("manual-continue", "/counters/manualContinueCount"),
            ] {
                let count = value
                    .pointer(pointer)
                    .and_then(|count| count.as_u64())
                    .unwrap_or(0);
                if count > 0 {
                    facts.counters.push(IndexedCounter {
                        owner_locator: task_locator.clone().unwrap_or_default(),
                        owner_type: "task",
                        activity_epoch: source.modified_epoch,
                        kind,
                        name: kind.to_string(),
                        count,
                    });
                }
            }
        }
        _ => {}
    }
    Ok(facts)
}

fn parse_jsonl(source: &SourceFile) -> Result<SourceFacts> {
    let reader = BufReader::new(File::open(&source.path)?);
    let mut facts = SourceFacts {
        status: "parsed",
        ..SourceFacts::default()
    };
    let task_locator = task_key(&source.relative);
    if source.file_type == "usage" {
        let Some(task_locator) = task_locator else {
            return Ok(facts);
        };
        let Some(attempt_locator) = attempt_key(&source.relative) else {
            return Ok(facts);
        };
        let Some(run_locator) = run_key(&source.relative) else {
            return Ok(facts);
        };
        let usage_facts = parse_prompt_usage(reader)?;
        let mut counters = TokenCounters::default();
        for usage in &usage_facts.completed_usages {
            counters.input = counters.input.saturating_add(usage.input);
            counters.output = counters.output.saturating_add(usage.output);
            counters.cache_read = counters.cache_read.saturating_add(usage.cache_read);
            counters.cache_write = counters.cache_write.saturating_add(usage.cache_write);
            counters.total = counters.total.saturating_add(effective_total(*usage));
        }
        let activity_epoch = usage_facts
            .turns
            .iter()
            .map(|turn| turn.activity_epoch)
            .max();
        if !usage_facts.completed_usages.is_empty() {
            facts.attempts.push(IndexedAttempt {
                attempt_locator: attempt_locator.clone(),
                run_locator,
                task_locator: task_locator.clone(),
                node_id: "unknown".to_string(),
                agent: None,
                outcome: None,
                child_run_locator: None,
                session_elapsed_seconds: None,
                activity_epoch: activity_epoch.unwrap_or(source.modified_epoch),
                token_usage: counters,
                observed_prompt_count: usage_facts.completed_usages.len() as u64,
            });
        }
        let mut direct_counts = BTreeMap::<(i64, String), u64>::new();
        for turn in usage_facts.turns {
            *direct_counts
                .entry((turn.activity_epoch, turn.status))
                .or_default() += 1;
        }
        for ((activity_epoch, status), count) in direct_counts {
            facts.counters.push(IndexedCounter {
                owner_locator: task_locator.clone(),
                owner_type: "task",
                activity_epoch,
                kind: "prompt-status",
                name: status,
                count,
            });
        }
    } else {
        let mut aggregate = BTreeMap::<(i64, &'static str, String), u64>::new();
        let mut latest_by_item = BTreeMap::<String, (u64, IndexedCounter)>::new();
        for line in reader.lines() {
            let value: serde_json::Value = serde_json::from_str(&line?)?;
            if value.get("patchType").is_some() {
                let patch: AcpTimelinePatch = serde_json::from_value(value)?;
                if patch.patch_type != "timelinePatch" || patch.op != "upsert" {
                    continue;
                }
                let item = serde_json::to_value(&patch.item)?;
                let counter = timeline_counter(
                    &item,
                    task_locator.as_deref().unwrap_or_default(),
                    source.modified_epoch,
                );
                let should_replace = latest_by_item
                    .get(&patch.item_id)
                    .map(|(revision, _)| patch.revision >= *revision)
                    .unwrap_or(true);
                if should_replace {
                    latest_by_item.insert(patch.item_id, (patch.revision, counter));
                }
                continue;
            }
            if let Ok(entry) = serde_json::from_value::<AcpTimelineItem>(value.clone())
                && !entry.item.id.is_empty()
            {
                let item_id = entry.item.id.clone();
                let should_replace = latest_by_item
                    .get(&item_id)
                    .map(|(revision, _)| *revision == 0)
                    .unwrap_or(true);
                if should_replace {
                    let item = serde_json::to_value(&entry.item)?;
                    latest_by_item.insert(
                        item_id,
                        (
                            0,
                            timeline_counter(
                                &item,
                                task_locator.as_deref().unwrap_or_default(),
                                source.modified_epoch,
                            ),
                        ),
                    );
                }
                continue;
            }
            let item = value.get("item").unwrap_or(&value);
            let counter = timeline_counter(
                item,
                task_locator.as_deref().unwrap_or_default(),
                string_at(&value, &["/timestamp", "/recordedAt"])
                    .and_then(timestamp_epoch)
                    .unwrap_or(source.modified_epoch),
            );
            *aggregate
                .entry((counter.activity_epoch, counter.kind, counter.name))
                .or_default() += 1;
        }
        for (_, counter) in latest_by_item.into_values() {
            *aggregate
                .entry((counter.activity_epoch, counter.kind, counter.name))
                .or_default() += 1;
        }
        for ((activity_epoch, kind, name), count) in aggregate {
            facts.counters.push(IndexedCounter {
                owner_locator: task_locator.clone().unwrap_or_default(),
                owner_type: "task",
                activity_epoch,
                kind,
                name,
                count,
            });
        }
    }
    Ok(facts)
}

fn timeline_counter(
    item: &serde_json::Value,
    task_locator: &str,
    fallback_activity_epoch: i64,
) -> IndexedCounter {
    let activity_epoch = string_at(item, &["/timestamp", "/recordedAt"])
        .and_then(timestamp_epoch)
        .unwrap_or(fallback_activity_epoch);
    let event_kind = item
        .get("kind")
        .and_then(|kind| kind.as_str())
        .unwrap_or("unknown");
    let (kind, name) = match event_kind {
        "toolCall" => ("tool", normalized_tool_name(item)),
        "permissionRequest" => ("permission", event_kind.to_string()),
        "elicitationRequest" => ("elicitation", event_kind.to_string()),
        "skillInvocation" => ("skill", normalized_tool_name(item)),
        _ => ("event", event_kind.to_string()),
    };
    IndexedCounter {
        owner_locator: task_locator.to_string(),
        owner_type: "task",
        activity_epoch,
        kind,
        name,
        count: 1,
    }
}

fn parse_semantic(source: &SourceFile) -> SourceFacts {
    let Ok(content) =
        read_bounded_utf8_prefix(&source.path, PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS)
    else {
        return SourceFacts {
            status: "corrupt",
            error_code: Some("analytics.source-corrupt".to_string()),
            ..SourceFacts::default()
        };
    };
    SourceFacts {
        status: "parsed",
        semantic: Some(IndexedSemantic {
            activity_epoch: source.modified_epoch,
            item: PersonalAnalyticsSemanticItem {
                locator: source.relative.clone(),
                kind: "requirement".to_string(),
                content,
            },
        }),
        ..SourceFacts::default()
    }
}

fn effective_total(usage: TokenCounters) -> u64 {
    if usage.total > 0 {
        usage.total
    } else {
        usage
            .input
            .saturating_add(usage.output)
            .saturating_add(usage.cache_read)
            .saturating_add(usage.cache_write)
    }
}

fn range_bounds(range: &PersonalAnalyticsDateRange) -> Result<RangeBounds> {
    let (Some(start_date), Some(end_date)) = (range.start.as_deref(), range.end.as_deref()) else {
        if range.start.is_none() && range.end.is_none() {
            return Ok(RangeBounds::All);
        }
        bail!("analytics.range-invalid");
    };
    let start_date = NaiveDate::parse_from_str(start_date, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?;
    let start_midnight = start_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("analytics.range-invalid"))?;
    let next_end_midnight = end_date
        .succ_opt()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .ok_or_else(|| anyhow::anyhow!("analytics.range-invalid"))?;
    let start = Local
        .from_local_datetime(&start_midnight)
        .earliest()
        .ok_or_else(|| anyhow::anyhow!("analytics.range-invalid"))?
        .timestamp();
    let end = Local
        .from_local_datetime(&next_end_midnight)
        .earliest()
        .ok_or_else(|| anyhow::anyhow!("analytics.range-invalid"))?
        .timestamp()
        - 1;
    if start > end {
        bail!("analytics.range-invalid");
    }
    Ok(RangeBounds::Bounded(start, end))
}

fn load_tasks(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "WITH task_activity(taskLocator, activityEpoch) AS (
             SELECT taskLocator, lastActivityEpoch FROM analytics_tasks
             WHERE lastActivityEpoch BETWEEN ?1 AND ?2
             UNION ALL
             SELECT taskLocator, activityEpoch FROM analytics_runs
             WHERE activityEpoch BETWEEN ?1 AND ?2
             UNION ALL
             SELECT taskLocator, activityEpoch FROM analytics_attempts
             WHERE activityEpoch BETWEEN ?1 AND ?2
             UNION ALL
             SELECT ownerLocator, activityEpoch FROM analytics_counters
             WHERE ownerType = 'task' AND activityEpoch BETWEEN ?1 AND ?2
             UNION ALL
             SELECT r.taskLocator, c.activityEpoch
             FROM analytics_counters c
             JOIN analytics_runs r ON c.ownerType = 'run' AND c.ownerLocator = r.runLocator
             WHERE c.activityEpoch BETWEEN ?1 AND ?2
             UNION ALL
             SELECT a.taskLocator, c.activityEpoch
             FROM analytics_counters c
             JOIN analytics_attempts a
               ON c.ownerType = 'attempt' AND c.ownerLocator = a.attemptLocator
             WHERE c.activityEpoch BETWEEN ?1 AND ?2
         ), scoped_tasks AS (
             SELECT t.taskLocator, t.title, t.mode, t.projectLocator,
                    MAX(a.activityEpoch) AS scopedActivityEpoch, t.conversationSourcePath
             FROM analytics_tasks t
             JOIN task_activity a ON a.taskLocator = t.taskLocator
             GROUP BY t.taskLocator, t.title, t.mode, t.projectLocator, t.conversationSourcePath
         ), ranked_runs AS (
             SELECT r.runLocator, r.taskLocator, r.unitType, r.status, r.outcome,
                    r.activityEpoch, r.terminalNode, r.pauseReason,
                    ROW_NUMBER() OVER (
                        PARTITION BY r.taskLocator
                        ORDER BY r.activityEpoch DESC, r.runLocator DESC
                    ) AS rank
             FROM analytics_runs r
             JOIN scoped_tasks t ON t.taskLocator=r.taskLocator
             WHERE r.unitType IN ('workflow-run', 'auto-outer-run', 'direct-session')
         )
         SELECT t.taskLocator, t.title, t.mode, t.projectLocator, t.scopedActivityEpoch,
                t.conversationSourcePath, r.runLocator, r.unitType, r.status, r.outcome,
                r.activityEpoch, r.terminalNode, r.pauseReason
         FROM scoped_tasks t
         LEFT JOIN ranked_runs r ON r.taskLocator=t.taskLocator AND r.rank=1",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
        ))
    })?;
    let mut projects = BTreeSet::new();
    let mut conversation_count = 0;
    for row in rows {
        let (
            locator,
            title,
            mode,
            project,
            last_activity,
            conversation_source,
            run_locator,
            run_unit_type,
            run_status,
            run_outcome,
            run_activity,
            run_terminal,
            run_pause,
        ) = row?;
        projects.insert(project);
        conversation_count += u64::from(conversation_source.is_some());
        let mut task = TaskFact {
            title,
            mode,
            ..TaskFact::default()
        };
        task.scoped_activity_epoch = last_activity;
        task.navigation_run = match (run_locator, run_unit_type, run_status) {
            (Some(run_locator), Some(unit_type), Some(status)) => Some(RunFact {
                task_key: locator.clone(),
                run_key: run_locator.clone(),
                unit_type,
                status,
                outcome: run_outcome,
                updated_epoch: run_activity,
                terminal_node: run_terminal,
                pause_reason: run_pause,
                locator: run_locator,
            }),
            _ => None,
        };
        accumulator.tasks.insert(locator, task);
    }
    accumulator.project_count = projects.len() as u64;
    accumulator.task_count = accumulator.tasks.len() as u64;
    accumulator.conversation_count = conversation_count;
    Ok(())
}

fn load_runs(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "SELECT runLocator, taskLocator, unitType, status, outcome, activityEpoch, terminalNode, pauseReason
         FROM analytics_runs WHERE activityEpoch BETWEEN ?1 AND ?2",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (locator, task, unit_type, status, outcome, epoch, terminal, pause) = row?;
        accumulator.runs.push(RunFact {
            task_key: task,
            run_key: locator.clone(),
            unit_type,
            status,
            outcome,
            updated_epoch: Some(epoch),
            terminal_node: terminal,
            pause_reason: pause,
            locator,
        });
        accumulator.earliest_epoch = Some(
            accumulator
                .earliest_epoch
                .map_or(epoch, |current| current.min(epoch)),
        );
        accumulator.latest_epoch = Some(
            accumulator
                .latest_epoch
                .map_or(epoch, |current| current.max(epoch)),
        );
    }
    Ok(())
}

fn load_semantic(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    accumulator.coverage.semantic_eligible_items = conn.query_row(
        "SELECT COUNT(*) FROM analytics_semantic_samples
         WHERE activityEpoch BETWEEN ?1 AND ?2",
        rusqlite::params![bounds.start(), bounds.end()],
        |row| row.get(0),
    )?;
    let mut statement = conn.prepare(
        "SELECT locator, kind, content FROM analytics_semantic_samples
         WHERE activityEpoch BETWEEN ?1 AND ?2 ORDER BY activityEpoch DESC, locator LIMIT ?3",
    )?;
    let rows = statement.query_map(
        rusqlite::params![
            bounds.start(),
            bounds.end(),
            PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS
        ],
        |row| {
            Ok(PersonalAnalyticsSemanticItem {
                locator: row.get(0)?,
                kind: row.get(1)?,
                content: row.get(2)?,
            })
        },
    )?;
    for row in rows {
        let mut item = row?;
        if accumulator.semantic_items.len() >= PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS
            || accumulator.semantic_chars >= PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS
        {
            break;
        }
        let remaining = PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS - accumulator.semantic_chars;
        item.content = item.content.chars().take(remaining).collect();
        let chars = item.content.chars().count();
        if chars == 0 {
            continue;
        }
        accumulator.semantic_chars += chars;
        accumulator.semantic_items.push(item);
        accumulator.coverage.semantic_sampled_items += 1;
    }
    Ok(())
}

fn load_coverage(
    conn: &Connection,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(sizeBytes), 0) FROM analytics_sources",
        [],
        |row| {
            accumulator.coverage.discovered_files = row.get(0)?;
            accumulator.coverage.discovered_bytes = row.get(1)?;
            Ok(())
        },
    )?;
    let mut statement =
        conn.prepare("SELECT parseStatus, COUNT(*) FROM analytics_sources GROUP BY parseStatus")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        let count = count.max(0) as u64;
        match status.as_str() {
            "parsed" | "unknown-version" => accumulator.coverage.parsed_files += count,
            "corrupt" => accumulator.coverage.corrupt_files += count,
            "skipped" => accumulator.coverage.skipped_files += count,
            _ => {}
        }
        if status != "skipped" {
            accumulator.coverage.eligible_files += count;
        }
        if status == "unknown-version" {
            accumulator.coverage.unknown_version_files += count;
        }
    }
    let mut evidence = conn.prepare(
        "SELECT sourcePath FROM analytics_sources WHERE parseStatus != 'skipped'
         ORDER BY sourcePath LIMIT 512",
    )?;
    accumulator.evidence = evidence
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn analytics_fixture(dir: &Path) {
        let task = dir.join("project-a/tasks/task-1");
        write(
            &task.join("task.json"),
            r#"{"version":"1.0","id":"task-1","title":"Indexed task"}"#,
        );
        write(
            &task.join("conversation.json"),
            r#"{"version":"1.0","runMode":"workflow","createdAt":"2026-08-18T01:00:00Z"}"#,
        );
        let run = task.join("runs/run-1");
        write(
            &run.join("run.json"),
            r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
        );
        let attempt = run.join("attempt-1");
        write(
            &attempt.join("node.json"),
            r#"{"version":"1.0","node_id":"plan","outcome":"success","resolved_config":{"provider":"agent-a"}}"#,
        );
        write(
            &attempt.join("acp.snapshot.json"),
            r#"{"version":"1.0","timing":{"sessionElapsedSeconds":60}}"#,
        );
        write(
            &attempt.join("acp.prompt-usage.jsonl"),
            concat!(
                r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:01Z"}"#,
                "\n",
                r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z","usage":{"inputTokens":100,"outputTokens":50,"cachedReadTokens":10,"totalTokens":160}}"#,
            ),
        );
        write(
            &attempt.join("acp.timeline.jsonl"),
            r#"{"item":{"kind":"toolCall","raw":{"name":"read_file"}}}"#,
        );
        write(
            &task.join("authoring/requirement.md"),
            "Build the indexed analytics report.",
        );
    }

    #[test]
    fn report_reads_state_and_facts_from_one_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let db = Utf8PathBuf::from_path_buf(temp.path().join("gold-band.db")).unwrap();
        let index = PersonalAnalyticsIndex::open(&db).unwrap();
        let transaction = index.conn.unchecked_transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO analytics_tasks VALUES (
                     'project-a/task-target', 'project-a', 'project', 'Snapshot before',
                     'workflow', NULL, NULL, 0)",
                [],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO analytics_runs VALUES (
                     'project-a/task-target/run-1', 'project-a/task-target', 'workflow-run',
                     'completed', 'success', 0, NULL, NULL, 'target')",
                [],
            )
            .unwrap();
        for task_number in 0..512 {
            let task_locator = format!("project-a/task-{task_number:03}");
            transaction
                .execute(
                    "INSERT INTO analytics_tasks VALUES (
                         ?1, 'project-a', 'project', ?2, 'workflow', NULL, NULL, 0)",
                    rusqlite::params![task_locator, format!("Task {task_number:03}")],
                )
                .unwrap();
            transaction
                .execute(
                    "INSERT INTO analytics_runs VALUES (
                         ?1, ?2, 'workflow-run', 'completed', 'success', 0, NULL, NULL, ?3)",
                    rusqlite::params![
                        format!("{task_locator}/run-1"),
                        task_locator,
                        format!("source-{task_number:03}")
                    ],
                )
                .unwrap();
        }
        transaction.commit().unwrap();

        let writer = Connection::open(db.as_std_path()).unwrap();
        writer
            .execute_batch(
                "PRAGMA busy_timeout=5000;
                 BEGIN IMMEDIATE;
                 UPDATE analytics_tasks
                 SET title = 'Snapshot after'
                 WHERE taskLocator = 'project-a/task-target';
                 UPDATE analytics_index_state SET indexRevision = 1 WHERE singleton = 1;",
            )
            .unwrap();
        let writer_slot = Mutex::new(Some(writer));
        let committed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler_committed = committed.clone();
        index.conn.progress_handler(
            100,
            Some(move || {
                if !handler_committed.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    writer_slot
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .execute_batch("COMMIT")
                        .unwrap();
                }
                false
            }),
        );
        let snapshot = index
            .report(&PersonalAnalyticsDateRange::default(), "snapshot".into())
            .unwrap();
        index.conn.progress_handler(100, None::<fn() -> bool>);
        assert!(committed.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(snapshot.index_revision, 0);
        assert_eq!(snapshot.overview.task_count, 513);
        assert!(
            snapshot
                .recent_tasks
                .iter()
                .any(|task| task.title == "Snapshot before")
        );
        assert!(
            !snapshot
                .recent_tasks
                .iter()
                .any(|task| task.title == "Snapshot after")
        );

        let updated = index
            .report(
                &PersonalAnalyticsDateRange::default(),
                "snapshot-updated".into(),
            )
            .unwrap();
        assert_eq!(updated.index_revision, 1);
        assert!(
            updated
                .recent_tasks
                .iter()
                .any(|task| task.title == "Snapshot after")
        );
    }

    #[test]
    fn schema_has_exactly_eight_physical_analytics_tables_and_views_aggregate() {
        let temp = tempfile::tempdir().unwrap();
        let db = Utf8PathBuf::from_path_buf(temp.path().join("gold-band.db")).unwrap();
        let index = PersonalAnalyticsIndex::open(&db).unwrap();
        let count: i64 = index
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE 'analytics_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, ANALYTICS_PHYSICAL_TABLES.len() as i64);
        for table in ANALYTICS_PHYSICAL_TABLES {
            let exists: bool = index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists);
        }
        for view in [
            "analytics_projects",
            "analytics_usage",
            "analytics_event_counts",
            "analytics_insights",
        ] {
            let exists: bool = index
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='view' AND name=?1)",
                    [view],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists);
        }
    }

    #[test]
    fn report_keeps_auto_tasks_navigable_and_classified() {
        let root = tempfile::tempdir().unwrap();
        let task = root.path().join("project-a/tasks/task-auto");
        write(
            &task.join("task.json"),
            r#"{"version":"1.0","id":"task-auto","title":"Auto task"}"#,
        );
        write(
            &task.join("conversation.json"),
            r#"{"version":"1.0","runMode":"auto","createdAt":"2026-08-18T01:00:00Z"}"#,
        );
        write(
            &task.join("runs/run-1/run.json"),
            r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
        );
        let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
        let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
        index.sync(&projects, |_, _| {}, || false).unwrap();
        let report = index
            .report(&PersonalAnalyticsDateRange::default(), "report-auto".into())
            .unwrap();

        assert_eq!(
            report
                .reliability
                .auto_outer_run_terminal_success_rate
                .numerator,
            1
        );
        assert_eq!(report.recent_tasks[0].mode, "auto");
        assert_eq!(
            report.recent_tasks[0].task_id,
            Some("task-auto".to_string())
        );
        assert_eq!(
            report.recent_tasks[0].latest_run_id,
            Some("run-1".to_string())
        );
    }

    #[test]
    fn incremental_sync_reports_ranges_and_views_from_index() {
        let root = tempfile::tempdir().unwrap();
        analytics_fixture(root.path());
        let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
        let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
        let first = index.sync(&projects, |_, _| {}, || false).unwrap();
        assert_eq!(first.reparsed_files, 8);
        let all = index
            .report(&PersonalAnalyticsDateRange::default(), "report-all".into())
            .unwrap();
        assert_eq!(
            all.reliability.workflow_run_terminal_success_rate.numerator,
            1
        );
        assert_eq!(
            all.recent_tasks[0].project_id,
            Some("project-a".to_string())
        );
        assert_eq!(all.recent_tasks[0].task_id, Some("task-1".to_string()));
        assert_eq!(all.recent_tasks[0].latest_run_id, Some("run-1".to_string()));
        assert_eq!(all.token_usage.total_tokens, 160);
        assert_eq!(all.efficiency.observed_terminal_run_active_seconds, 60);
        assert_eq!(all.index_revision, first.index_revision);
        let empty = index
            .report(
                &PersonalAnalyticsDateRange {
                    start: Some("2026-01-01".into()),
                    end: Some("2026-01-31".into()),
                },
                "report-range".into(),
            )
            .unwrap();
        assert_eq!(
            empty
                .reliability
                .workflow_run_terminal_success_rate
                .denominator,
            0
        );
        assert_eq!(empty.token_usage.total_tokens, 0);

        let unchanged = index.sync(&projects, |_, _| {}, || false).unwrap();
        assert_eq!(unchanged.reparsed_files, 0);
        assert_eq!(unchanged.index_revision, first.index_revision);
        let usage = projects
            .as_std_path()
            .join("project-a/tasks/task-1/runs/run-1/attempt-1/acp.prompt-usage.jsonl");
        write(
            &usage,
            concat!(
                r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z"}"#,
                "\n",
                r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:03Z","usage":{"inputTokens":300,"outputTokens":100,"totalTokens":400}}"#,
            ),
        );
        let changed = index.sync(&projects, |_, _| {}, || false).unwrap();
        assert_eq!(changed.reparsed_files, 1);
        let updated = index
            .report(
                &PersonalAnalyticsDateRange::default(),
                "report-updated".into(),
            )
            .unwrap();
        assert_eq!(updated.token_usage.total_tokens, 400);
        std::fs::remove_file(
            projects
                .as_std_path()
                .join("project-a/tasks/task-1/runs/run-1/run.json"),
        )
        .unwrap();
        let deleted = index.sync(&projects, |_, _| {}, || false).unwrap();
        assert_eq!(deleted.deleted_files, 1);
        assert!(
            index
                .connection()
                .query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM analytics_runs WHERE unitType='workflow-run')",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
        let usage_total: i64 = index
            .connection()
            .query_row(
                "SELECT SUM(totalTokens) FROM analytics_usage WHERE taskLocator='project-a/task-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(usage_total, 400);
    }

    #[test]
    fn report_history_bounds_use_min_max_for_out_of_order_runs() {
        let temp = tempfile::tempdir().unwrap();
        let db = Utf8PathBuf::from_path_buf(temp.path().join("gold-band.db")).unwrap();
        let index = PersonalAnalyticsIndex::open(&db).unwrap();
        index
            .connection()
            .execute(
                "INSERT INTO analytics_tasks VALUES ('project-a/task-1','project-a','Project A','Task 1','workflow',NULL,NULL,NULL)",
                [],
            )
            .unwrap();
        for (locator, epoch) in [("run-middle", 20), ("run-early", 10), ("run-late", 30)] {
            index
                .connection()
                .execute(
                    "INSERT INTO analytics_runs VALUES (?1,'project-a/task-1','workflow-run','completed','success',?2,NULL,NULL,?1)",
                    rusqlite::params![locator, epoch],
                )
                .unwrap();
        }

        let report = index
            .report(&PersonalAnalyticsDateRange::default(), "bounds".into())
            .unwrap();
        assert_eq!(
            report.overview.earliest_at,
            Some("1970-01-01T00:00:10Z".to_string())
        );
        assert_eq!(
            report.overview.latest_at,
            Some("1970-01-01T00:00:30Z".to_string())
        );
    }

    #[test]
    fn counter_constraints_and_insight_cache_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let db = Utf8PathBuf::from_path_buf(temp.path().join("gold-band.db")).unwrap();
        let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
        assert!(index.connection().execute(
            "INSERT INTO analytics_counters VALUES ('source','run','run',0,'unsupported-kind','name',1)", []
        ).is_err());
        let identity = InsightIdentity {
            operation_id: "operation-1".into(),
            range_start: None,
            range_end: None,
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.into(),
            index_revision: 1,
            agent_type: "agent-a".into(),
            model_id: None,
            thought_level_option_id: None,
            thought_level_value: None,
        };
        let narrative = PersonalAnalyticsNarrative {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.into(),
            insights: Vec::new(),
        };
        index
            .store_completed_insight(&identity, &narrative, "2026-08-18T00:01:00Z")
            .unwrap();
        assert!(index.completed_insight(&identity).unwrap().is_some());
        let view_count: i64 = index
            .connection()
            .query_row("SELECT COUNT(*) FROM analytics_insights", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(view_count, 0);
    }
}

fn load_attempts(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "SELECT attemptLocator, runLocator, taskLocator, nodeSourcePath, snapshotSourcePath,
                nodeId, agent, outcome, childRunLocator, sessionElapsedSeconds, activityEpoch,
                inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, totalTokens,
                observedPromptCount
         FROM analytics_attempts WHERE activityEpoch BETWEEN ?1 AND ?2",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, i64>(15)?,
            row.get::<_, i64>(16)?,
        ))
    })?;
    for row in rows {
        let (
            attempt,
            run,
            task,
            node_source,
            snapshot_source,
            node_id,
            agent,
            outcome,
            child_run_key,
            seconds,
            _epoch,
            input,
            output,
            cache_read,
            cache_write,
            total,
            observed_prompt_count,
        ) = row?;
        let execution_attempt = node_source.is_some() || snapshot_source.is_some();
        if execution_attempt {
            accumulator.attempt_count += 1;
        }
        if let Some(agent) = agent {
            accumulator
                .tasks
                .entry(task.clone())
                .or_default()
                .agent_names
                .insert(agent.clone());
            *accumulator.agent_counts.entry(agent).or_default() += 1;
        }
        if node_source.is_some() {
            accumulator.nodes.push(NodeFact {
                task_key: task.clone(),
                run_key: run,
                attempt_key: attempt.clone(),
                group_key: node_group_key(&attempt).unwrap_or_else(|| attempt.clone()),
                node_id,
                outcome,
                child_run_key,
            });
        }
        if let Some(seconds) = seconds.map(|value| value.max(0) as u64) {
            accumulator
                .session_duration_seconds_by_attempt
                .insert(attempt, seconds);
        }
        let usage = TokenCounters {
            input: input.max(0) as u64,
            output: output.max(0) as u64,
            cache_read: cache_read.max(0) as u64,
            cache_write: cache_write.max(0) as u64,
            total: total.max(0) as u64,
        };
        add_index_usage(
            accumulator,
            task,
            usage,
            observed_prompt_count.max(0) as u64,
        );
    }
    Ok(())
}

fn add_index_usage(
    accumulator: &mut ProjectionAccumulator,
    task: String,
    usage: TokenCounters,
    observed_prompt_count: u64,
) {
    let total = effective_total(usage);
    let target = accumulator.tasks.entry(task).or_default();
    target.token_usage.input += usage.input;
    target.token_usage.output += usage.output;
    target.token_usage.cache_read += usage.cache_read;
    target.token_usage.cache_write += usage.cache_write;
    target.token_usage.total += total;
    accumulator.token_usage.input_tokens += usage.input;
    accumulator.token_usage.output_tokens += usage.output;
    accumulator.token_usage.cache_read_tokens += usage.cache_read;
    accumulator.token_usage.cache_write_tokens += usage.cache_write;
    accumulator.token_usage.total_tokens += total;
    accumulator.token_usage.observed_prompt_count += observed_prompt_count;
}

fn load_counters(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    let mut direct_statement = conn.prepare(
        "SELECT c.ownerLocator, c.name, c.sourcePath, c.count
         FROM analytics_counters c
         JOIN analytics_tasks t ON t.taskLocator = c.ownerLocator
         WHERE c.kind = 'prompt-status' AND t.mode = 'direct'
           AND c.activityEpoch BETWEEN ?1 AND ?2",
    )?;
    let direct_rows =
        direct_statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
            Ok(DirectReplyFact {
                task_key: row.get(0)?,
                status: row.get(1)?,
                evidence_locator: row.get(2)?,
                count: row.get::<_, i64>(3)?.max(0) as u64,
            })
        })?;
    for row in direct_rows {
        accumulator.direct_replies.push(row?);
    }
    let mut statement = conn.prepare(
        "SELECT c.kind, c.name, SUM(c.count) FROM analytics_counters c
         WHERE c.kind != 'prompt-status' AND c.activityEpoch BETWEEN ?1 AND ?2
         GROUP BY c.kind, c.name",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, name, count) = row?;
        let count = count.max(0) as u64;
        match kind.as_str() {
            "tool" => *accumulator.tool_counts.entry(name).or_default() += count,
            "permission" => accumulator.permission_request_count += count,
            "elicitation" => accumulator.elicitation_request_count += count,
            "skill" => *accumulator.skill_counts.entry(name).or_default() += count,
            "pause" => accumulator.pause_count += count,
            "resume" => accumulator.resume_count += count,
            "manual-continue" => accumulator.manual_continue_count += count,
            _ => *accumulator.event_counts.entry(name).or_default() += count,
        }
    }
    Ok(())
}
