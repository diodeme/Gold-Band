use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{Local, NaiveDate, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::{
    NodeFact, PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION, PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS,
    PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS, PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS,
    PersonalAnalyticsNarrative, PersonalAnalyticsReport, PersonalAnalyticsSemanticItem,
    ProjectionAccumulator, RunFact, TaskFact, TokenCounters, attempt_key,
    canonicalize_personal_analytics_report, finalize_projection, is_excluded, node_group_key,
    normalized_tool_name, run_key, safe_relative_path, string_at, task_key, timestamp_epoch,
    token_counters, u64_at_optional,
};

const ANALYTICS_INDEX_SCHEMA_VERSION: i64 = 1;
const ANALYTICS_INSIGHT_RUNS_RETAINED: i64 = 64;
#[cfg(test)]
const ANALYTICS_PHYSICAL_TABLES: [&str; 8] = [
    "analytics_sources",
    "analytics_index_state",
    "analytics_tasks",
    "analytics_runs",
    "analytics_attempts",
    "analytics_counters",
    "analytics_semantic_samples",
    "analytics_insight_runs",
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
    session_elapsed_seconds: Option<u64>,
    activity_epoch: i64,
    token_usage: TokenCounters,
}

#[derive(Debug, Clone, PartialEq)]
struct IndexedCounter {
    owner_locator: String,
    owner_type: &'static str,
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
                unitType TEXT NOT NULL CHECK (unitType IN ('workflow-run','auto-outer-run','direct-reply')),
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
                nodeId TEXT NOT NULL,
                agent TEXT,
                sessionElapsedSeconds INTEGER,
                zeroFilled INTEGER NOT NULL,
                activityEpoch INTEGER NOT NULL,
                inputTokens INTEGER NOT NULL,
                outputTokens INTEGER NOT NULL,
                cacheReadTokens INTEGER NOT NULL,
                cacheWriteTokens INTEGER NOT NULL,
                totalTokens INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS analytics_attempts_range_idx ON analytics_attempts(activityEpoch, runLocator, taskLocator);
            CREATE TABLE IF NOT EXISTS analytics_counters (
                sourcePath TEXT NOT NULL,
                ownerType TEXT NOT NULL CHECK (ownerType IN ('run','attempt','task')),
                ownerLocator TEXT NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('tool','permission','elicitation','pause','resume','manual-continue','skill','event','agent','direct-status')),
                name TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (sourcePath, ownerType, ownerLocator, kind, name)
            );
            CREATE INDEX IF NOT EXISTS analytics_counters_owner_idx ON analytics_counters(ownerLocator, kind, name);
            CREATE TABLE IF NOT EXISTS analytics_semantic_samples (
                sourcePath TEXT PRIMARY KEY,
                locator TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                activityEpoch INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS analytics_insight_runs (
                operationId TEXT PRIMARY KEY,
                rangeStart TEXT,
                rangeEnd TEXT,
                schemaVersion TEXT NOT NULL,
                indexRevision INTEGER NOT NULL,
                agentType TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('processing','completed','failed','cancelled')),
                errorCode TEXT,
                insightsJson TEXT NOT NULL,
                createdAt TEXT NOT NULL,
                updatedAt TEXT NOT NULL
            );
            DROP INDEX IF EXISTS analytics_insight_cache_idx;
            CREATE UNIQUE INDEX IF NOT EXISTS analytics_insight_completed_cache_idx
                ON analytics_insight_runs(rangeStart, rangeEnd, schemaVersion, indexRevision, agentType)
                WHERE status = 'completed';
            CREATE VIEW IF NOT EXISTS analytics_projects AS
                SELECT projectLocator, projectName, COUNT(DISTINCT taskLocator) AS taskCount,
                       MIN(lastActivityEpoch) AS firstActivityEpoch, MAX(lastActivityEpoch) AS lastActivityEpoch
                FROM analytics_tasks GROUP BY projectLocator, projectName;
            CREATE VIEW IF NOT EXISTS analytics_usage AS
                SELECT taskLocator, SUM(inputTokens) AS inputTokens, SUM(outputTokens) AS outputTokens,
                       SUM(cacheReadTokens) AS cacheReadTokens, SUM(cacheWriteTokens) AS cacheWriteTokens,
                       SUM(totalTokens) AS totalTokens, COUNT(*) AS attemptCount
                FROM analytics_attempts GROUP BY taskLocator;
            CREATE VIEW IF NOT EXISTS analytics_event_counts AS
                SELECT kind, name, SUM(count) AS count FROM analytics_counters GROUP BY kind, name;
            CREATE VIEW IF NOT EXISTS analytics_insights AS
                SELECT operationId, value->>'$.section' AS section, value->>'$.title' AS title
                FROM analytics_insight_runs, json_each(insightsJson, '$.insights');
            INSERT OR IGNORE INTO analytics_index_state(singleton, schemaVersion, indexRevision, syncStatus)
                VALUES (1, 1, 0, 'idle');
            "#,
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
        let files = discover_sources(&canonical_root)?;
        let existing = self.existing_sources()?;
        let total = files.len() as u64;
        let mut pending = Vec::new();
        let mut reparsed_files = 0u64;
        let progress = Mutex::new(progress);
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
        let replacements = Self::parse_sources_parallel(pending, total, &progress, &cancelled)?;
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
        let tx = self.conn.transaction()?;
        mark_sync(&tx, "syncing", None)?;
        for path in &deleted_files {
            delete_source_facts(&tx, path)?;
            tx.execute(
                "DELETE FROM analytics_sources WHERE sourcePath = ?1",
                [path],
            )?;
        }
        for (source, facts) in &replacements {
            upsert_source(&tx, source, facts)?;
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
        Ok(AnalyticsIndexStats {
            index_revision,
            changed_files: reparsed_files + deleted_files.len() as u64,
            reparsed_files,
            deleted_files: deleted_files.len() as u64,
            duration_ms: started.elapsed().as_millis() as u64,
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
        let bounds = range_bounds(range)?;
        let state = self.state()?;
        let mut accumulator = ProjectionAccumulator::default();
        load_tasks(&self.conn, bounds, &mut accumulator)?;
        load_runs(&self.conn, bounds, &mut accumulator)?;
        load_attempts(&self.conn, bounds, &mut accumulator)?;
        load_counters(&self.conn, bounds, &mut accumulator)?;
        load_semantic(&self.conn, bounds, &mut accumulator)?;
        load_coverage(&self.conn, &mut accumulator)?;
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
        Ok(report)
    }

    pub fn semantic_batch(
        &self,
        range: &PersonalAnalyticsDateRange,
    ) -> Result<Vec<PersonalAnalyticsSemanticItem>> {
        let bounds = range_bounds(range)?;
        let mut statement = self.conn.prepare(
            "SELECT locator, kind, content FROM analytics_semantic_samples
             WHERE activityEpoch BETWEEN ?1 AND ?2 ORDER BY activityEpoch DESC, locator LIMIT ?3",
        )?;
        let items = statement
            .query_map(
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
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn completed_insight(
        &self,
        identity: &InsightIdentity,
    ) -> rusqlite::Result<Option<PersonalAnalyticsNarrative>> {
        let payload = self
            .conn
            .query_row(
                "SELECT insightsJson FROM analytics_insight_runs
                 WHERE rangeStart IS ?1 AND rangeEnd IS ?2 AND schemaVersion = ?3
                   AND indexRevision = ?4 AND agentType = ?5 AND status = 'completed'
                 ORDER BY updatedAt DESC LIMIT 1",
                rusqlite::params![
                    identity.range_start,
                    identity.range_end,
                    identity.schema_version,
                    identity.index_revision,
                    identity.agent_type
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

    pub fn begin_insight(&self, identity: &InsightIdentity, now: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO analytics_insight_runs
             (operationId, rangeStart, rangeEnd, schemaVersion, indexRevision, agentType,
              status, errorCode, insightsJson, createdAt, updatedAt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'processing', NULL, '{}', ?7, ?7)",
            rusqlite::params![
                identity.operation_id,
                identity.range_start,
                identity.range_end,
                identity.schema_version,
                identity.index_revision,
                identity.agent_type,
                now
            ],
        )?;
        self.prune_insight_runs(Some(&identity.operation_id))?;
        Ok(())
    }

    pub fn finish_insight(
        &self,
        operation_id: &str,
        narrative: &PersonalAnalyticsNarrative,
        status: &'static str,
        error_code: Option<&str>,
        now: &str,
    ) -> rusqlite::Result<()> {
        let payload = serde_json::to_string(narrative)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        self.conn.execute(
            "UPDATE analytics_insight_runs
             SET status = ?2, errorCode = ?3, insightsJson = ?4, updatedAt = ?5
             WHERE operationId = ?1",
            rusqlite::params![operation_id, status, error_code, payload, now],
        )?;
        self.prune_insight_runs(None)?;
        Ok(())
    }

    fn prune_insight_runs(&self, keep_operation_id: Option<&str>) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM analytics_insight_runs
             WHERE (?1 IS NULL OR operationId != ?1)
               AND operationId NOT IN (
                   SELECT operationId FROM analytics_insight_runs
                   ORDER BY updatedAt DESC LIMIT ?2
               )",
            rusqlite::params![keep_operation_id, ANALYTICS_INSIGHT_RUNS_RETAINED],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
}

fn drop_analytics_schema(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "DROP VIEW IF EXISTS analytics_projects;
         DROP VIEW IF EXISTS analytics_usage;
         DROP VIEW IF EXISTS analytics_event_counts;
         DROP VIEW IF EXISTS analytics_insights;
         DROP TABLE IF EXISTS analytics_insight_runs;
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
) -> rusqlite::Result<()> {
    tx.execute(
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
    delete_source_facts(tx, &source.relative)?;
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
        tx.execute(
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
        refresh_run_types(tx, &update.task_locator)?;
    }
    for run in &facts.runs {
        tx.execute(
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
    }
    for attempt in &facts.attempts {
        let zero_filled = attempt.session_elapsed_seconds.is_none();
        tx.execute(
            "INSERT INTO analytics_attempts VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(attemptLocator) DO UPDATE SET
               runLocator = excluded.runLocator,
               taskLocator = excluded.taskLocator,
               nodeId = CASE WHEN excluded.nodeId = 'unknown' THEN analytics_attempts.nodeId ELSE excluded.nodeId END,
               agent = COALESCE(excluded.agent, analytics_attempts.agent),
               sessionElapsedSeconds = CASE WHEN excluded.sessionElapsedSeconds IS NOT NULL THEN excluded.sessionElapsedSeconds ELSE analytics_attempts.sessionElapsedSeconds END,
               zeroFilled = CASE WHEN excluded.sessionElapsedSeconds IS NOT NULL THEN 0 ELSE analytics_attempts.zeroFilled END,
               activityEpoch = excluded.activityEpoch,
               inputTokens = analytics_attempts.inputTokens + excluded.inputTokens,
               outputTokens = analytics_attempts.outputTokens + excluded.outputTokens,
               cacheReadTokens = analytics_attempts.cacheReadTokens + excluded.cacheReadTokens,
               cacheWriteTokens = analytics_attempts.cacheWriteTokens + excluded.cacheWriteTokens,
               totalTokens = analytics_attempts.totalTokens + excluded.totalTokens",
            rusqlite::params![
                attempt.attempt_locator,
                attempt.run_locator,
                attempt.task_locator,
                attempt.node_id,
                attempt.agent,
                attempt.session_elapsed_seconds,
                zero_filled,
                attempt.activity_epoch,
                attempt.token_usage.input,
                attempt.token_usage.output,
                attempt.token_usage.cache_read,
                attempt.token_usage.cache_write,
                attempt.token_usage.total
            ],
        )?;
    }
    for counter in &facts.counters {
        tx.execute(
            "INSERT OR REPLACE INTO analytics_counters VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                source.relative,
                counter.owner_type,
                counter.owner_locator,
                counter.kind,
                counter.name,
                counter.count
            ],
        )?;
    }
    if let Some(semantic) = &facts.semantic {
        tx.execute(
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
    Ok(())
}

fn delete_source_facts(tx: &Transaction<'_>, source_path: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM analytics_runs WHERE sourcePath = ?1",
        [source_path],
    )?;
    tx.execute(
        "DELETE FROM analytics_attempts
         WHERE attemptLocator = ?1 OR attemptLocator LIKE ?1 || '#usage-%'",
        [source_path],
    )?;
    tx.execute(
        "UPDATE analytics_attempts SET sessionElapsedSeconds = NULL, zeroFilled = 1
         WHERE attemptLocator = REPLACE(?1, '/acp.snapshot.json', '')",
        [source_path],
    )?;
    tx.execute(
        "DELETE FROM analytics_counters WHERE sourcePath = ?1",
        [source_path],
    )?;
    tx.execute(
        "DELETE FROM analytics_semantic_samples WHERE sourcePath = ?1",
        [source_path],
    )?;
    tx.execute(
        "UPDATE analytics_tasks SET title = taskLocator, taskSourcePath = NULL WHERE taskSourcePath = ?1",
        [source_path],
    )?;
    tx.execute(
        "UPDATE analytics_tasks SET mode = 'unknown', conversationSourcePath = NULL,
                lastActivityEpoch = NULL WHERE conversationSourcePath = ?1",
        [source_path],
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

fn refresh_run_types(tx: &Transaction<'_>, task_locator: &str) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE analytics_runs SET unitType = CASE
           WHEN unitType = 'direct-reply' THEN 'direct-reply'
           WHEN (SELECT mode FROM analytics_tasks WHERE taskLocator = ?1) = 'auto' THEN 'auto-outer-run'
           ELSE 'workflow-run'
         END WHERE taskLocator = ?1",
        [task_locator],
    )?;
    Ok(())
}

fn discover_sources(root: &Utf8Path) -> Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
            .map_err(|path| anyhow::anyhow!("invalid UTF-8 analytics path: {}", path.display()))?;
        let Some(relative) = safe_relative_path(root, &path) else {
            continue;
        };
        let file_name = path.file_name().unwrap_or_default();
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
        let file_type = source_type(file_name);
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

fn source_type(file_name: &str) -> &'static str {
    match file_name {
        "project.json" => "project",
        "task.json" => "task",
        "conversation.json" => "conversation",
        "run.json" => "run",
        "node.json" => "node",
        "turn.json" => "turn",
        "acp.snapshot.json" => "snapshot",
        "observability.snapshot.json" => "observability",
        "acp.prompt-usage.jsonl" => "usage",
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
        "turn" => {
            let task_locator = task_locator.unwrap_or_default();
            let run_locator = format!(
                "{task_locator}/direct/{}",
                source.relative.rsplit('/').next().unwrap_or("turn")
            );
            let data = value.pointer("/record/data").unwrap_or(&value);
            let status = data
                .get("status")
                .and_then(|status| status.as_str())
                .unwrap_or("unknown");
            facts.runs.push(IndexedRun {
                run_locator: run_locator.clone(),
                task_locator,
                unit_type: "direct-reply".to_string(),
                status: status.to_string(),
                outcome: data
                    .get("outcome")
                    .and_then(|outcome| outcome.as_str())
                    .map(str::to_string),
                activity_epoch: string_at(
                    &value,
                    &["/record/data/finishedAt", "/record/updatedAt", "/updatedAt"],
                )
                .and_then(timestamp_epoch)
                .unwrap_or(source.modified_epoch),
                terminal_node: None,
                pause_reason: None,
            });
            facts.counters.push(IndexedCounter {
                owner_locator: run_locator,
                owner_type: "run",
                kind: "direct-status",
                name: status.to_string(),
                count: 1,
            });
        }
        "node" => {
            let task_locator = task_locator.unwrap_or_default();
            facts.attempts.push(IndexedAttempt {
                attempt_locator: attempt_key(&source.relative)
                    .unwrap_or_else(|| source.relative.clone()),
                run_locator: run_key(&source.relative).unwrap_or_default(),
                task_locator,
                node_id: value
                    .get("node_id")
                    .and_then(|node| node.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                agent: value
                    .pointer("/resolved_config/provider")
                    .and_then(|agent| agent.as_str())
                    .map(str::to_string),
                session_elapsed_seconds: None,
                activity_epoch: source.modified_epoch,
                token_usage: TokenCounters::default(),
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
                session_elapsed_seconds: u64_at_optional(
                    &value,
                    &["/timing/sessionElapsedSeconds"],
                ),
                activity_epoch: source.modified_epoch,
                token_usage: TokenCounters::default(),
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
        for (index, line) in reader.lines().enumerate() {
            let value: serde_json::Value = serde_json::from_str(&line?)?;
            let Some(usage) = value.get("usage") else {
                continue;
            };
            let turn = value
                .get("turn_id")
                .or_else(|| value.get("turnId"))
                .and_then(|turn| turn.as_str())
                .unwrap_or("unknown");
            let mut counters = token_counters(usage);
            counters.total = effective_total(counters);
            facts.attempts.push(IndexedAttempt {
                attempt_locator: format!("{}#usage-{index}", source.relative),
                run_locator: format!("{}/direct/{turn}", task_locator.clone().unwrap_or_default()),
                task_locator: task_locator.clone().unwrap_or_default(),
                node_id: "direct-reply".to_string(),
                agent: None,
                session_elapsed_seconds: None,
                activity_epoch: string_at(&value, &["/timestamp", "/recordedAt"])
                    .and_then(timestamp_epoch)
                    .unwrap_or(source.modified_epoch),
                token_usage: counters,
            });
        }
    } else {
        let mut aggregate = BTreeMap::<(&'static str, String), u64>::new();
        for line in reader.lines() {
            let value: serde_json::Value = serde_json::from_str(&line?)?;
            let item = value.get("item").unwrap_or(&value);
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
            *aggregate.entry((kind, name)).or_default() += 1;
        }
        for ((kind, name), count) in aggregate {
            facts.counters.push(IndexedCounter {
                owner_locator: task_locator.clone().unwrap_or_default(),
                owner_type: "task",
                kind,
                name,
                count,
            });
        }
    }
    Ok(facts)
}

fn parse_semantic(source: &SourceFile) -> SourceFacts {
    let mut content = String::new();
    let read = File::open(&source.path).and_then(|file| {
        file.take((PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS * 4) as u64)
            .read_to_string(&mut content)
    });
    if read.is_err() {
        return SourceFacts {
            status: "corrupt",
            error_code: Some("analytics.source-corrupt".to_string()),
            ..SourceFacts::default()
        };
    }
    let content = content
        .chars()
        .take(PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS)
        .collect::<String>();
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
        "SELECT DISTINCT t.taskLocator, t.title, t.mode, t.projectLocator, t.lastActivityEpoch
         FROM analytics_tasks t
         WHERE EXISTS (SELECT 1 FROM analytics_runs r
                       WHERE r.taskLocator=t.taskLocator AND r.activityEpoch BETWEEN ?1 AND ?2)
            OR EXISTS (SELECT 1 FROM analytics_attempts a
                       WHERE a.taskLocator=t.taskLocator AND a.activityEpoch BETWEEN ?1 AND ?2)",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<i64>>(4)?,
        ))
    })?;
    let mut projects = BTreeSet::new();
    for row in rows {
        let (locator, title, mode, project, last_activity) = row?;
        projects.insert(project);
        let mut task = TaskFact {
            title,
            mode,
            ..TaskFact::default()
        };
        task.last_activity_epoch = last_activity;
        accumulator.tasks.insert(locator, task);
    }
    accumulator.project_count = projects.len() as u64;
    accumulator.task_count = accumulator.tasks.len() as u64;
    accumulator.conversation_count = accumulator
        .tasks
        .values()
        .filter(|task| task.mode == "direct")
        .count() as u64;
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
        if unit_type == "direct-reply" {
            accumulator.direct_started += 1;
            match status.as_str() {
                "completed" => accumulator.direct_completed += 1,
                "failed" => accumulator.direct_failed += 1,
                "cancelled" | "canceled" => accumulator.direct_cancelled += 1,
                _ => accumulator.direct_unknown += 1,
            }
        }
        accumulator.runs.push(RunFact {
            task_key: task,
            run_key: locator,
            status,
            outcome,
            updated_epoch: Some(epoch),
            terminal_node: terminal,
            pause_reason: pause,
            locator: String::new(),
        });
        accumulator.earliest_epoch =
            Some(accumulator.earliest_epoch.unwrap_or(epoch)).filter(|value| *value <= epoch);
        accumulator.latest_epoch =
            Some(accumulator.latest_epoch.unwrap_or(epoch)).filter(|value| *value >= epoch);
    }
    Ok(())
}

fn load_semantic(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
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
        let item = row?;
        accumulator.coverage.semantic_eligible_items += 1;
        let chars = item.content.chars().count();
        if accumulator.semantic_items.len() >= PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS
            || accumulator.semantic_chars + chars > PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS
        {
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
            &task.join("acp.prompt-usage.jsonl"),
            r#"{"usage":{"inputTokens":100,"outputTokens":50,"cachedReadTokens":10,"totalTokens":160}}"#,
        );
        write(
            &task.join("acp.timeline.jsonl"),
            r#"{"item":{"kind":"toolCall","raw":{"name":"read_file"}}}"#,
        );
        write(
            &task.join("authoring/requirement.md"),
            "Build the indexed analytics report.",
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
    fn incremental_sync_reports_ranges_and_views_from_index() {
        let root = tempfile::tempdir().unwrap();
        analytics_fixture(root.path());
        let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
        let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
        let first = index.sync(&projects, |_, _| {}, || false).unwrap();
        assert_eq!(first.reparsed_files, 9);
        let all = index
            .report(&PersonalAnalyticsDateRange::default(), "report-all".into())
            .unwrap();
        assert_eq!(
            all.reliability.workflow_run_terminal_success_rate.numerator,
            1
        );
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
            .join("project-a/tasks/task-1/acp.prompt-usage.jsonl");
        write(
            &usage,
            r#"{"usage":{"inputTokens":300,"outputTokens":100,"totalTokens":400}}"#,
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
    fn counter_constraints_and_insight_cache_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let db = Utf8PathBuf::from_path_buf(temp.path().join("gold-band.db")).unwrap();
        let index = PersonalAnalyticsIndex::open(&db).unwrap();
        assert!(index.connection().execute(
            "INSERT INTO analytics_counters VALUES ('source','run','run','unsupported-kind','name',1)", []
        ).is_err());
        let identity = InsightIdentity {
            operation_id: "operation-1".into(),
            range_start: None,
            range_end: None,
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.into(),
            index_revision: 1,
            agent_type: "agent-a".into(),
        };
        index
            .begin_insight(&identity, "2026-08-18T00:00:00Z")
            .unwrap();
        let narrative = PersonalAnalyticsNarrative {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.into(),
            insights: Vec::new(),
        };
        index
            .finish_insight(
                "operation-1",
                &narrative,
                "completed",
                None,
                "2026-08-18T00:01:00Z",
            )
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
        "SELECT attemptLocator, runLocator, taskLocator, nodeId, agent,
                sessionElapsedSeconds, activityEpoch, inputTokens, outputTokens,
                cacheReadTokens, cacheWriteTokens, totalTokens
         FROM analytics_attempts WHERE activityEpoch BETWEEN ?1 AND ?2",
    )?;
    let rows = statement.query_map(rusqlite::params![bounds.start(), bounds.end()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
        ))
    })?;
    for row in rows {
        let (
            attempt,
            run,
            task,
            node_id,
            agent,
            seconds,
            _epoch,
            input,
            output,
            cache_read,
            cache_write,
            total,
        ) = row?;
        accumulator.attempt_count += 1;
        if let Some(agent) = agent {
            accumulator
                .tasks
                .entry(task.clone())
                .or_default()
                .agent_names
                .insert(agent.clone());
            *accumulator.agent_counts.entry(agent).or_default() += 1;
        }
        accumulator.nodes.push(NodeFact {
            task_key: task.clone(),
            run_key: run,
            attempt_key: attempt.clone(),
            group_key: node_group_key(&attempt).unwrap_or_else(|| attempt.clone()),
            node_id,
            outcome: None,
        });
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
        add_index_usage(accumulator, task, usage);
    }
    Ok(())
}

fn add_index_usage(accumulator: &mut ProjectionAccumulator, task: String, usage: TokenCounters) {
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
    accumulator.token_usage.observed_prompt_count += 1;
}

fn load_counters(
    conn: &Connection,
    bounds: RangeBounds,
    accumulator: &mut ProjectionAccumulator,
) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "SELECT c.kind, c.name, SUM(c.count) FROM analytics_counters c
         WHERE EXISTS (SELECT 1 FROM analytics_runs r
                       WHERE (r.runLocator=c.ownerLocator OR r.taskLocator=c.ownerLocator)
                         AND r.activityEpoch BETWEEN ?1 AND ?2)
            OR EXISTS (SELECT 1 FROM analytics_attempts a
                       WHERE a.attemptLocator=c.ownerLocator AND a.activityEpoch BETWEEN ?1 AND ?2)
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
