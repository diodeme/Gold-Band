use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use rusqlite::{Connection, params};
use tracing::warn;

use crate::acp::events::{AcpSessionMetadata, load_timeline_items};
use crate::runtime::TaskState;
use crate::storage::read_json;

// ── global singleton ─────────────────────────────────────────────────

static SEARCH_INDEX: OnceLock<Arc<SearchIndex>> = OnceLock::new();

pub fn init_search_index(
    db_path: &Utf8Path,
    projects_dir: &Utf8Path,
) -> Result<Arc<SearchIndex>, rusqlite::Error> {
    let index = Arc::new(SearchIndex::open(db_path)?);

    // If the DB is empty (first run), backfill from existing files in a
    // background thread so startup is not delayed.
    if index.is_empty() {
        let index_clone = index.clone();
        let projects_dir = projects_dir.to_path_buf();
        std::thread::spawn(move || {
            index_clone.backfill_from_disk(&projects_dir);
        });
    }

    let _ = SEARCH_INDEX.set(index.clone());
    Ok(index)
}

pub fn search_index() -> Option<&'static Arc<SearchIndex>> {
    SEARCH_INDEX.get()
}

// ── attempt indexing context ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AttemptIndexContext {
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug)]
struct LoadedSessionIndexCandidate {
    attempt_dir: Utf8PathBuf,
    attempt_path: String,
    context: AttemptIndexContext,
    session_id: Option<String>,
    status: &'static str,
    title: String,
    created_at: String,
    updated_at: String,
    prompts: Vec<LoadedPromptIndexCandidate>,
}

#[derive(Debug)]
struct LoadedPromptIndexCandidate {
    id: String,
    prompt_id: Option<String>,
    timestamp: String,
    text: String,
    normalized_text: String,
}

/// Convenience: index an attempt with retry, using the global search index.
/// Call this from any `spawn_blocking` context after files are written.
/// No-op if the search index hasn't been initialized.
pub fn index_attempt_with_retry(attempt_dir: &Utf8Path, ctx: &AttemptIndexContext) {
    let Some(index) = search_index() else {
        return;
    };
    index.index_session_with_retry(attempt_dir, ctx);
}

/// Convenience: index a task with retry, using the global search index.
/// Reads `task.json` and `authoring/requirement.md` from `task_dir`.
/// No-op if the search index hasn't been initialized.
pub fn index_task_with_retry(task_dir: &Utf8Path, task_id: &str) {
    let Some(index) = search_index() else {
        return;
    };
    index.index_task_with_retry(task_dir, task_id);
}

pub fn delete_task(task_dir: &Utf8Path) {
    let Some(index) = search_index() else {
        return;
    };
    if let Err(error) = index.delete_task(task_dir) {
        warn!("sqlite delete_task failed for {}: {:#}", task_dir, error);
    }
}

// ── SearchIndex ──────────────────────────────────────────────────────

const MAX_RETRIES: u32 = 3;
const RETRY_DELAYS_MS: [u64; 3] = [200, 500, 1500];
const SEARCH_INDEX_SCHEMA_VERSION: i32 = 6;
const DELETE_RUN_PROMPTS_SQL: &str = "DELETE FROM session_prompts
     WHERE attempt_path = ?1
        OR (attempt_path >= ?2 AND attempt_path < ?3)";
const DELETE_RUN_SESSIONS_SQL: &str = "DELETE FROM sessions
     WHERE attempt_path = ?1
        OR (attempt_path >= ?2 AND attempt_path < ?3)";

/// Best-effort SQLite search index for cross-session prompt/timeline retrieval.
///
/// **Consistency model**: files are the authoritative source. Writes to SQLite happen
/// *after* files are successfully written. DB write failures are retried up to
/// `MAX_RETRIES` times with fresh file reads each attempt, then silently dropped
/// (logged via `tracing::warn`). Deleting the DB file has zero impact on session
/// detail, recovery, or diagnostics — a lazy backfill can rebuild it.
///
/// **Thread safety**: the internal `Mutex<Connection>` is held only for the
/// duration of each insert/query, never across file I/O. All DB access should
/// go through `spawn_blocking`.
pub struct SearchIndex {
    conn: Mutex<Connection>,
}

impl SearchIndex {
    pub fn open(db_path: &Utf8Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(db_path.as_std_path())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=3000;")?;
        let index = Self {
            conn: Mutex::new(conn),
        };
        index.ensure_schema()?;
        Ok(index)
    }

    // ── schema ──────────────────────────────────────────────────

    fn ensure_schema(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        let schema_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if schema_version != SEARCH_INDEX_SCHEMA_VERSION {
            warn!(
                "sqlite search index schema version mismatch (found {}, expected {})",
                schema_version, SEARCH_INDEX_SCHEMA_VERSION
            );
        }

        let mut task_schema_migrated = false;
        let tasks_table_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
            [],
            |row| row.get(0),
        )?;
        if tasks_table_exists {
            let task_path_is_primary_key = {
                let mut stmt = conn.prepare("PRAGMA table_info(tasks)")?;
                let columns = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(1)?, row.get::<_, i32>(5)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                columns
                    .iter()
                    .any(|(name, primary_key)| name == "task_path" && *primary_key > 0)
            };
            if !task_path_is_primary_key {
                task_schema_migrated = true;
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(
                    "DROP TRIGGER IF EXISTS tasks_ai;
                    DROP TRIGGER IF EXISTS tasks_ad;
                    DROP TRIGGER IF EXISTS tasks_au;
                    DROP TABLE IF EXISTS tasks_fts;
                    ALTER TABLE tasks RENAME TO tasks_legacy;
                    CREATE TABLE tasks (
                        task_id      TEXT NOT NULL,
                        task_path    TEXT NOT NULL PRIMARY KEY,
                        title        TEXT NOT NULL DEFAULT '',
                        description  TEXT NOT NULL DEFAULT '',
                        requirement_text TEXT NOT NULL DEFAULT '',
                        created_at   TEXT NOT NULL DEFAULT '',
                        updated_at   TEXT NOT NULL DEFAULT ''
                    );
                    INSERT OR REPLACE INTO tasks (
                        task_id, task_path, title, description, requirement_text, created_at, updated_at
                    )
                    SELECT task_id, task_path, title, description, requirement_text, created_at, updated_at
                    FROM tasks_legacy;
                    DROP TABLE tasks_legacy;",
                )?;
                tx.commit()?;
            }
        }

        let rebuild_task_fts = tasks_table_exists && schema_version != SEARCH_INDEX_SCHEMA_VERSION;
        if rebuild_task_fts && !task_schema_migrated {
            conn.execute_batch(
                "DROP TRIGGER IF EXISTS tasks_ai;
                DROP TRIGGER IF EXISTS tasks_ad;
                DROP TRIGGER IF EXISTS tasks_au;
                DROP TABLE IF EXISTS tasks_fts;",
            )?;
        }

        if schema_version != SEARCH_INDEX_SCHEMA_VERSION {
            conn.execute_batch(
                "DROP TRIGGER IF EXISTS session_prompts_ai;
                DROP TRIGGER IF EXISTS session_prompts_ad;
                DROP TRIGGER IF EXISTS session_prompts_au;
                DROP TABLE IF EXISTS session_prompts_fts;
                DROP TABLE IF EXISTS session_prompts;
                DROP TABLE IF EXISTS sessions;",
            )?;
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                task_id      TEXT NOT NULL,
                task_path    TEXT NOT NULL PRIMARY KEY,
                title        TEXT NOT NULL DEFAULT '',
                description  TEXT NOT NULL DEFAULT '',
                requirement_text TEXT NOT NULL DEFAULT '',
                created_at   TEXT NOT NULL DEFAULT '',
                updated_at   TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS sessions (
                session_id   TEXT,
                attempt_path TEXT NOT NULL PRIMARY KEY,
                task_id      TEXT NOT NULL,
                run_id       TEXT NOT NULL,
                round_id     TEXT NOT NULL,
                node_id      TEXT NOT NULL,
                attempt_id   TEXT NOT NULL,
                outer_node_id     TEXT,
                outer_attempt_id  TEXT,
                title        TEXT,
                status       TEXT NOT NULL DEFAULT '',
                created_at   TEXT NOT NULL DEFAULT '',
                updated_at   TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS session_prompts (
                id            TEXT NOT NULL,
                attempt_path  TEXT NOT NULL,
                session_id    TEXT,
                prompt_id     TEXT,
                timestamp     TEXT NOT NULL DEFAULT '',
                text          TEXT NOT NULL DEFAULT '',
                normalized_text TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (attempt_path, id)
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS session_prompts_fts
                USING fts5(text, content=session_prompts, content_rowid=rowid);

            CREATE TRIGGER IF NOT EXISTS session_prompts_ai AFTER INSERT ON session_prompts BEGIN
                INSERT INTO session_prompts_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            CREATE TRIGGER IF NOT EXISTS session_prompts_ad AFTER DELETE ON session_prompts BEGIN
                INSERT INTO session_prompts_fts(session_prompts_fts, rowid, text) VALUES('delete', old.rowid, old.text);
            END;
            CREATE TRIGGER IF NOT EXISTS session_prompts_au AFTER UPDATE ON session_prompts BEGIN
                INSERT INTO session_prompts_fts(session_prompts_fts, rowid, text) VALUES('delete', old.rowid, old.text);
                INSERT INTO session_prompts_fts(rowid, text) VALUES (new.rowid, new.text);
            END;

            CREATE VIRTUAL TABLE IF NOT EXISTS tasks_fts
                USING fts5(
                    title,
                    description,
                    requirement_text,
                    content=tasks,
                    content_rowid=rowid,
                    tokenize='trigram'
                );

            CREATE TRIGGER IF NOT EXISTS tasks_ai AFTER INSERT ON tasks BEGIN
                INSERT INTO tasks_fts(rowid, title, description, requirement_text)
                VALUES (new.rowid, new.title, new.description, new.requirement_text);
            END;
            CREATE TRIGGER IF NOT EXISTS tasks_ad AFTER DELETE ON tasks BEGIN
                INSERT INTO tasks_fts(tasks_fts, rowid, title, description, requirement_text)
                VALUES('delete', old.rowid, old.title, old.description, old.requirement_text);
            END;
            CREATE TRIGGER IF NOT EXISTS tasks_au AFTER UPDATE ON tasks BEGIN
                INSERT INTO tasks_fts(tasks_fts, rowid, title, description, requirement_text)
                VALUES('delete', old.rowid, old.title, old.description, old.requirement_text);
                INSERT INTO tasks_fts(rowid, title, description, requirement_text)
                VALUES (new.rowid, new.title, new.description, new.requirement_text);
            END;",
        )?;
        if task_schema_migrated || rebuild_task_fts {
            conn.execute("INSERT INTO tasks_fts(tasks_fts) VALUES('rebuild')", [])?;
        }
        conn.execute_batch(&format!(
            "PRAGMA user_version = {};",
            SEARCH_INDEX_SCHEMA_VERSION
        ))?;
        Ok(())
    }

    // ── backfill ───────────────────────────────────────────────

    fn is_empty(&self) -> bool {
        let conn = self.conn.lock().expect("search index lock poisoned");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap_or(0);
        count == 0
    }

    /// Walk all project directories under `projects_dir`, reading
    /// `task.json` / `requirement.md` for tasks and `acp.snapshot.json` /
    /// `acp.timeline.jsonl` for attempts, upserting into the DB.
    ///
    /// This is idempotent (`ON CONFLICT` upsert) and runs on the calling
    /// thread — call from `std::thread::spawn` to avoid blocking startup.
    fn backfill_from_disk(&self, projects_dir: &Utf8Path) {
        if let Err(error) = self.backfill_from_disk_strict(projects_dir) {
            warn!(error = %error, "sqlite search index backfill did not complete");
        }
    }

    pub fn rebuild_from_disk(&self, projects_dir: &Utf8Path) -> anyhow::Result<()> {
        {
            let conn = self
                .conn
                .lock()
                .map_err(|_| anyhow::anyhow!("search index lock poisoned"))?;
            let transaction = conn.unchecked_transaction()?;
            transaction.execute("DELETE FROM session_prompts", [])?;
            transaction.execute("DELETE FROM sessions", [])?;
            transaction.execute("DELETE FROM tasks", [])?;
            transaction.commit()?;
        }
        self.backfill_from_disk_strict(projects_dir)
    }

    fn backfill_from_disk_strict(&self, projects_dir: &Utf8Path) -> anyhow::Result<()> {
        if !projects_dir.is_dir() {
            return Ok(());
        }
        for project_entry in std::fs::read_dir(projects_dir.as_std_path())? {
            let project_entry = project_entry?;
            let Some(tasks_dir) = to_utf8(project_entry.path().join("tasks")) else {
                continue;
            };
            if !tasks_dir.is_dir() {
                continue;
            }
            for task_entry in std::fs::read_dir(tasks_dir.as_std_path())? {
                let task_entry = task_entry?;
                let Some(task_dir) = to_utf8(task_entry.path()) else {
                    continue;
                };
                if !task_dir.is_dir() {
                    continue;
                }
                let Some(task_id) = file_name(&task_dir) else {
                    continue;
                };
                self.index_task(&task_dir, task_id)?;
                self.backfill_task_attempts(&task_dir, task_id)?;
            }
        }
        Ok(())
    }

    fn backfill_task_attempts(&self, task_dir: &Utf8Path, task_id: &str) -> anyhow::Result<()> {
        let runs_dir = task_dir.join("runs");
        if !runs_dir.is_dir() {
            return Ok(());
        }
        for run_entry in std::fs::read_dir(runs_dir.as_std_path())? {
            let run_entry = run_entry?;
            let Some(run_dir) = to_utf8(run_entry.path()) else {
                continue;
            };
            if !run_dir.is_dir() {
                continue;
            }
            let Some(run_id) = file_name(&run_dir) else {
                continue;
            };

            let rounds_dir = run_dir.join("rounds");
            if !rounds_dir.is_dir() {
                continue;
            }
            for round_entry in std::fs::read_dir(rounds_dir.as_std_path())? {
                let round_entry = round_entry?;
                let Some(round_dir) = to_utf8(round_entry.path()) else {
                    continue;
                };
                if !round_dir.is_dir() {
                    continue;
                }
                let Some(round_id) = file_name(&round_dir) else {
                    continue;
                };

                let nodes_dir = round_dir.join("nodes");
                if !nodes_dir.is_dir() {
                    continue;
                }
                for node_entry in std::fs::read_dir(nodes_dir.as_std_path())? {
                    let node_entry = node_entry?;
                    let Some(node_dir) = to_utf8(node_entry.path()) else {
                        continue;
                    };
                    if !node_dir.is_dir() {
                        continue;
                    }
                    let Some(node_id) = file_name(&node_dir) else {
                        continue;
                    };

                    for attempt_entry in std::fs::read_dir(node_dir.as_std_path())? {
                        let attempt_entry = attempt_entry?;
                        let Some(attempt_dir) = to_utf8(attempt_entry.path()) else {
                            continue;
                        };
                        if !attempt_dir.is_dir() {
                            continue;
                        }
                        if !attempt_dir.join("acp.snapshot.json").exists() {
                            continue;
                        }
                        let Some(attempt_id) = file_name(&attempt_dir) else {
                            continue;
                        };
                        let ctx = AttemptIndexContext {
                            task_id: task_id.to_string(),
                            run_id: run_id.to_string(),
                            round_id: round_id.to_string(),
                            node_id: node_id.to_string(),
                            attempt_id: attempt_id.to_string(),
                            outer_node_id: None,
                            outer_attempt_id: None,
                        };
                        self.index_session(&attempt_dir, &ctx)?;
                    }
                }
            }
        }
        Ok(())
    }

    // ── index with retry ────────────────────────────────────────

    /// Index a session attempt. Each retry re-reads `acp.snapshot.json` and
    /// `acp.timeline.jsonl` fresh from disk, so the write always uses the
    /// latest state even if the session was still streaming during earlier
    /// attempts.
    pub fn index_session_with_retry(&self, attempt_dir: &Utf8Path, ctx: &AttemptIndexContext) {
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(RETRY_DELAYS_MS[attempt as usize]));
            }
            match self.index_session(attempt_dir, ctx) {
                Ok(()) => return,
                Err(e) => {
                    warn!(
                        "sqlite index_session failed (attempt {}/{}): {:#}",
                        attempt + 1,
                        MAX_RETRIES,
                        e
                    );
                }
            }
        }
    }

    fn index_session(
        &self,
        attempt_dir: &Utf8Path,
        ctx: &AttemptIndexContext,
    ) -> Result<(), rusqlite::Error> {
        let candidate = load_session_index_candidate(attempt_dir, ctx);
        self.commit_session_index_candidate(candidate)
    }

    fn commit_session_index_candidate(
        &self,
        candidate: LoadedSessionIndexCandidate,
    ) -> Result<(), rusqlite::Error> {
        let LoadedSessionIndexCandidate {
            attempt_dir,
            attempt_path,
            context,
            session_id,
            status,
            title,
            created_at,
            updated_at,
            prompts,
        } = candidate;
        let conn = self.conn.lock().expect("search index lock poisoned");
        if !attempt_dir.is_dir() {
            return Ok(());
        }
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO sessions
                (session_id, attempt_path, task_id, run_id, round_id,
                 node_id, attempt_id, outer_node_id, outer_attempt_id,
                 title, status, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(attempt_path) DO UPDATE SET
                session_id=excluded.session_id,
                title=excluded.title,
                status=excluded.status,
                updated_at=excluded.updated_at",
            params![
                session_id,
                attempt_path,
                context.task_id,
                context.run_id,
                context.round_id,
                context.node_id,
                context.attempt_id,
                context.outer_node_id,
                context.outer_attempt_id,
                title,
                status,
                created_at,
                updated_at,
            ],
        )?;

        for prompt in prompts {
            tx.execute(
                "INSERT INTO session_prompts
                    (id, attempt_path, session_id, prompt_id, timestamp, text, normalized_text)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(attempt_path, id) DO UPDATE SET
                    session_id=excluded.session_id,
                    text=excluded.text,
                    normalized_text=excluded.normalized_text",
                params![
                    prompt.id,
                    attempt_path,
                    session_id,
                    prompt.prompt_id,
                    prompt.timestamp,
                    prompt.text,
                    prompt.normalized_text,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    // ── search ──────────────────────────────────────────────────

    pub fn search_prompts(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PromptSearchResult>, rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        let normalized = normalize_for_search(query);
        let mut stmt = conn.prepare(
            "SELECT sp.id, sp.session_id, sp.prompt_id, sp.timestamp, sp.text,
                    s.attempt_path, s.task_id, s.run_id, s.round_id, s.node_id,
                    s.attempt_id, s.outer_node_id, s.outer_attempt_id, s.title
             FROM session_prompts_fts fts
             JOIN session_prompts sp ON fts.rowid = sp.rowid
             JOIN sessions s ON s.attempt_path = sp.attempt_path
             WHERE session_prompts_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![normalized, limit as i64], |row| {
            Ok(PromptSearchResult {
                prompt_event_id: row.get(0)?,
                session_id: row.get(1)?,
                prompt_id: row.get(2)?,
                timestamp: row.get(3)?,
                text: row.get(4)?,
                attempt_path: row.get(5)?,
                task_id: row.get(6)?,
                run_id: row.get(7)?,
                round_id: row.get(8)?,
                node_id: row.get(9)?,
                attempt_id: row.get(10)?,
                outer_node_id: row.get(11)?,
                outer_attempt_id: row.get(12)?,
                session_title: row.get(13)?,
            })
        })?;
        rows.collect()
    }

    // ── task indexing ──────────────────────────────────────────

    pub fn index_task_with_retry(&self, task_dir: &Utf8Path, task_id: &str) {
        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(RETRY_DELAYS_MS[attempt as usize]));
            }
            match self.index_task(task_dir, task_id) {
                Ok(()) => return,
                Err(e) => {
                    warn!(
                        "sqlite index_task failed (attempt {}/{}): {:#}",
                        attempt + 1,
                        MAX_RETRIES,
                        e
                    );
                }
            }
        }
    }

    fn index_task(&self, task_dir: &Utf8Path, task_id: &str) -> Result<(), rusqlite::Error> {
        let task_path = task_dir.to_string();
        let task: Option<TaskState> = read_json(&task_dir.join("task.json")).ok();
        let requirement_text = std::fs::read_to_string(
            task_dir
                .join("authoring")
                .join("requirement.md")
                .as_std_path(),
        )
        .unwrap_or_default();

        let (title, description, created_at, updated_at) = task
            .as_ref()
            .map(|t| {
                (
                    t.title.as_deref().unwrap_or(""),
                    t.description.as_deref().unwrap_or(""),
                    "", // TaskState has no created_at; snapshot-based timestamps don't apply here
                    "", // We could derive from file mtime, but keep it simple
                )
            })
            .unwrap_or(("", "", "", ""));

        let conn = self.conn.lock().expect("search index lock poisoned");
        conn.execute(
            "INSERT INTO tasks (task_id, task_path, title, description, requirement_text, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(task_path) DO UPDATE SET
                task_id=excluded.task_id,
                title=excluded.title,
                description=excluded.description,
                requirement_text=excluded.requirement_text,
                updated_at=excluded.updated_at",
            params![task_id, task_path, title, description, requirement_text, created_at, updated_at],
        )?;
        Ok(())
    }

    // ── task search ────────────────────────────────────────────

    pub fn delete_task(&self, task_dir: &Utf8Path) -> Result<(), rusqlite::Error> {
        let task_path = task_dir.to_string();
        let conn = self.conn.lock().expect("search index lock poisoned");
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM session_prompts WHERE attempt_path IN (
                SELECT attempt_path FROM sessions WHERE attempt_path LIKE (?1 || '%')
            )",
            params![&task_path],
        )?;
        tx.execute(
            "DELETE FROM sessions WHERE attempt_path LIKE (?1 || '%')",
            params![&task_path],
        )?;
        tx.execute(
            "DELETE FROM tasks WHERE task_path = ?1",
            params![&task_path],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_run(&self, run_dir: &Utf8Path) -> Result<(), rusqlite::Error> {
        let run_path = crate::storage::normalize_workspace_path(run_dir);
        self.delete_run_by_normalized_path(&run_path)
    }

    pub fn delete_run_by_normalized_path(&self, run_path: &str) -> Result<(), rusqlite::Error> {
        let descendant_start = format!("{run_path}/");
        let descendant_end = format!("{run_path}0");
        let conn = self.conn.lock().expect("search index lock poisoned");
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            DELETE_RUN_PROMPTS_SQL,
            params![run_path, descendant_start, descendant_end],
        )?;
        tx.execute(
            DELETE_RUN_SESSIONS_SQL,
            params![run_path, descendant_start, descendant_end],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn search_tasks(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<TaskSearchResult>, rusqlite::Error> {
        self.search_tasks_with_scope(query, None, limit)
    }

    /// Search tasks whose indexed path belongs to one of the supplied task roots.
    ///
    /// Scope filtering is part of the SQL query so out-of-scope rows cannot consume
    /// the result limit before callers assemble workspace-specific view models.
    pub fn search_tasks_in_task_roots(
        &self,
        query: &str,
        task_roots: &[String],
        limit: usize,
    ) -> Result<Vec<TaskSearchResult>, rusqlite::Error> {
        if task_roots.is_empty() {
            return Ok(Vec::new());
        }

        self.search_tasks_with_scope(query, Some(task_roots), limit)
    }

    fn search_tasks_with_scope(
        &self,
        query: &str,
        task_roots: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<TaskSearchResult>, rusqlite::Error> {
        let normalized = normalize_for_search(query);
        let terms = normalized.split_whitespace().collect::<Vec<_>>();
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().expect("search index lock poisoned");
        let use_trigram = terms.iter().all(|term| term.chars().count() >= 3);
        let task_path_prefixes = task_roots
            .unwrap_or_default()
            .iter()
            .map(|root| {
                let root = root.trim_end_matches(['/', '\\']);
                format!("{root}{}", std::path::MAIN_SEPARATOR)
            })
            .collect::<Vec<_>>();
        let query_parameter_count = if use_trigram { 1 } else { terms.len() };
        let scope_sql = task_path_prefixes
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let parameter = query_parameter_count + index + 1;
                if cfg!(windows) {
                    format!(
                        "substr(t.task_path, 1, length(?{parameter})) = ?{parameter} COLLATE NOCASE"
                    )
                } else {
                    format!("substr(t.task_path, 1, length(?{parameter})) = ?{parameter}")
                }
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        let scope_clause = if scope_sql.is_empty() {
            String::new()
        } else {
            format!(" AND ({scope_sql})")
        };
        let limit_parameter = query_parameter_count + task_path_prefixes.len() + 1;
        let (from_and_match, order_by) = if use_trigram {
            (
                "FROM tasks_fts fts
                 JOIN tasks t ON fts.rowid = t.rowid
                 WHERE tasks_fts MATCH ?1"
                    .to_string(),
                "ORDER BY bm25(tasks_fts, 10.0, 3.0, 1.0)".to_string(),
            )
        } else {
            let term_clauses = terms
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let parameter = index + 1;
                    format!(
                        "(instr(lower(t.title), ?{parameter}) > 0
                          OR instr(lower(t.description), ?{parameter}) > 0
                          OR instr(lower(t.requirement_text), ?{parameter}) > 0)"
                    )
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            (
                format!("FROM tasks t WHERE {term_clauses}"),
                "ORDER BY CASE
                    WHEN instr(lower(t.title), ?1) > 0 THEN 0
                    WHEN instr(lower(t.description), ?1) > 0 THEN 1
                    ELSE 2
                 END,
                 t.rowid DESC"
                    .to_string(),
            )
        };
        let sql = format!(
            "SELECT t.task_id, t.task_path, t.title, t.description,
                    substr(t.requirement_text, 1, 500),
                    t.requirement_text
             {from_and_match}
             {scope_clause}
             {order_by}
             LIMIT ?{limit_parameter}"
        );
        let mut stmt = conn.prepare(&sql)?;
        if use_trigram {
            stmt.raw_bind_parameter(1, compile_literal_fts_query(&terms))?;
        } else {
            for (index, term) in terms.iter().enumerate() {
                stmt.raw_bind_parameter(index + 1, term)?;
            }
        }
        for (index, prefix) in task_path_prefixes.iter().enumerate() {
            stmt.raw_bind_parameter(query_parameter_count + index + 1, prefix)?;
        }
        stmt.raw_bind_parameter(limit_parameter, limit as i64)?;

        let mut rows = stmt.raw_query();
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let title: String = row.get(2)?;
            let description: String = row.get(3)?;
            let requirement_text: String = row.get(5)?;
            results.push(TaskSearchResult {
                task_id: row.get(0)?,
                task_path: row.get(1)?,
                match_preview: task_match_preview(&title, &description, &requirement_text, &terms),
                title,
                description,
                requirement_preview: row.get(4)?,
            });
        }
        Ok(results)
    }

    // ── session search ─────────────────────────────────────────

    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchResult>, rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut stmt = conn.prepare(
            "SELECT session_id, attempt_path, task_id, run_id, round_id, node_id,
                    attempt_id, outer_node_id, outer_attempt_id, title, status,
                    created_at, updated_at
             FROM sessions
             WHERE title LIKE ?1 ESCAPE '\\'
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(SessionSearchResult {
                session_id: row.get(0)?,
                attempt_path: row.get(1)?,
                task_id: row.get(2)?,
                run_id: row.get(3)?,
                round_id: row.get(4)?,
                node_id: row.get(5)?,
                attempt_id: row.get(6)?,
                outer_node_id: row.get(7)?,
                outer_attempt_id: row.get(8)?,
                title: row.get(9)?,
                status: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?;
        rows.collect()
    }
}

// ── helpers ──────────────────────────────────────────────────────────

fn load_session_index_candidate(
    attempt_dir: &Utf8Path,
    context: &AttemptIndexContext,
) -> LoadedSessionIndexCandidate {
    let attempt_path = crate::storage::normalize_workspace_path(attempt_dir);
    let snapshot = read_snapshot(attempt_dir);
    let (session_id, status, title, created_at, updated_at) = snapshot
        .map(|snapshot| {
            (
                snapshot.session_id,
                match snapshot.latest_turn_status {
                    crate::acp::events::AcpLatestTurnStatus::None => "none",
                    crate::acp::events::AcpLatestTurnStatus::Completed => "completed",
                    crate::acp::events::AcpLatestTurnStatus::Cancelled => "cancelled",
                    crate::acp::events::AcpLatestTurnStatus::Failed => "failed",
                },
                snapshot.title.unwrap_or_default(),
                snapshot.created_at,
                snapshot.updated_at,
            )
        })
        .unwrap_or((None, "", String::new(), String::new(), String::new()));
    let prompts = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl"))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            if item.kind != "userTextDelta" {
                return None;
            }
            let text = item.content?;
            if text.trim().is_empty() {
                return None;
            }
            let prompt_id = item
                .raw
                .as_ref()
                .and_then(|raw| raw.get("promptId"))
                .and_then(|value| value.as_str())
                .map(String::from);
            Some(LoadedPromptIndexCandidate {
                id: item.id,
                prompt_id,
                timestamp: item.timestamp,
                normalized_text: normalize_for_search(&text),
                text,
            })
        })
        .collect();

    LoadedSessionIndexCandidate {
        attempt_dir: attempt_dir.to_path_buf(),
        attempt_path,
        context: context.clone(),
        session_id,
        status,
        title,
        created_at,
        updated_at,
        prompts,
    }
}

fn read_snapshot(attempt_dir: &Utf8Path) -> Option<AcpSessionMetadata> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    if snapshot_path.exists() {
        return crate::acp::events::load_session_metadata(&snapshot_path, None).ok();
    }
    let session_path = attempt_dir.join("acp.session.json");
    if session_path.exists() {
        return crate::acp::events::load_session_metadata(&session_path, None).ok();
    }
    None
}

fn to_utf8(path: std::path::PathBuf) -> Option<camino::Utf8PathBuf> {
    camino::Utf8PathBuf::from_path_buf(path).ok()
}

fn file_name(path: &camino::Utf8Path) -> Option<&str> {
    let name = path.file_name()?;
    if name.is_empty() { None } else { Some(name) }
}

fn normalize_for_search(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.extend(ch.to_lowercase());
            prev_ws = false;
        }
    }
    out.trim().to_string()
}

fn compile_literal_fts_query(terms: &[&str]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn task_match_preview(
    title: &str,
    description: &str,
    requirement_text: &str,
    terms: &[&str],
) -> String {
    let compact_title = compact_search_preview(title);
    let compact_description = compact_search_preview(description);
    let compact_requirement = compact_search_preview(requirement_text);
    for term in terms {
        for candidate in [&compact_title, &compact_description, &compact_requirement] {
            if let Some(preview) = excerpt_around_search_term(candidate, term) {
                return preview;
            }
        }
    }
    compact_requirement.chars().take(96).collect::<String>()
}

fn compact_search_preview(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn excerpt_around_search_term(text: &str, term: &str) -> Option<String> {
    const CONTEXT_BEFORE_CHARS: usize = 10;
    const MAX_PREVIEW_CHARS: usize = 96;

    let normalized_text = text.to_lowercase();
    let normalized_term = term.to_lowercase();
    let match_byte = normalized_text.find(&normalized_term)?;
    let match_start = normalized_text[..match_byte].chars().count();
    let match_length = normalized_term.chars().count();
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= MAX_PREVIEW_CHARS {
        return Some(text.to_string());
    }
    let start = match_start.saturating_sub(CONTEXT_BEFORE_CHARS);
    let minimum_end = match_start.saturating_add(match_length);
    let end = start
        .saturating_add(MAX_PREVIEW_CHARS)
        .max(minimum_end)
        .min(chars.len());
    let mut preview = chars[start..end].iter().collect::<String>();
    if start > 0 {
        preview.insert(0, '…');
    }
    if end < chars.len() {
        preview.push('…');
    }
    Some(preview)
}

// ── search result types ─────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptSearchResult {
    pub prompt_event_id: String,
    pub session_id: Option<String>,
    pub prompt_id: Option<String>,
    pub timestamp: String,
    pub text: String,
    pub attempt_path: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub session_title: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSearchResult {
    pub task_id: String,
    pub task_path: String,
    pub title: String,
    pub description: String,
    /// First 500 chars of requirement content for search result preview
    pub requirement_preview: String,
    /// Context excerpt selected from the field that matched the current query.
    pub match_preview: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResult {
    pub session_id: Option<String>,
    pub attempt_path: String,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rebuilds_v5_search_index_schema_for_normalized_attempt_paths() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();

        {
            let conn = Connection::open(db_path.as_std_path()).unwrap();
            conn.execute_batch(
                "CREATE TABLE tasks (
                    task_id TEXT NOT NULL PRIMARY KEY,
                    task_path TEXT NOT NULL,
                    title TEXT NOT NULL DEFAULT '',
                    description TEXT NOT NULL DEFAULT '',
                    requirement_text TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT ''
                );
                INSERT INTO tasks (task_id, task_path, title, description, requirement_text, created_at, updated_at)
                VALUES ('task-1', '/tmp/task-1', 'Task 1', '', '', '', '');
                CREATE TABLE sessions (
                    session_id TEXT,
                    adapter_id TEXT NOT NULL DEFAULT '',
                    attempt_path TEXT NOT NULL PRIMARY KEY
                );
                INSERT INTO sessions (session_id, adapter_id, attempt_path)
                VALUES ('session-real-123', 'npx', '/tmp/attempt-1');
                CREATE TABLE session_prompts (
                    id TEXT NOT NULL PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    prompt_id TEXT,
                    timestamp TEXT NOT NULL DEFAULT '',
                    text TEXT NOT NULL DEFAULT '',
                    normalized_text TEXT NOT NULL DEFAULT ''
                );
                PRAGMA user_version = 5;",
            )
            .unwrap();
        }

        let index = SearchIndex::open(&db_path).unwrap();
        let conn = index.conn.lock().unwrap();
        let schema_version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(schema_version, SEARCH_INDEX_SCHEMA_VERSION);

        let task_fts_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'tasks_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(task_fts_sql.contains("tokenize='trigram'"));

        let mut stmt = conn.prepare("PRAGMA table_info(session_prompts)").unwrap();
        let prompt_columns = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i32>(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(
            prompt_columns
                .iter()
                .any(|(name, _)| name == "attempt_path")
        );
        assert!(
            prompt_columns
                .iter()
                .any(|(name, not_null)| name == "session_id" && *not_null == 0)
        );

        let mut session_stmt = conn.prepare("PRAGMA table_info(sessions)").unwrap();
        let session_columns = session_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i32>(3)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            session_columns
                .iter()
                .any(|(name, not_null)| name == "session_id" && *not_null == 0)
        );
        assert!(!session_columns.iter().any(|(name, _)| name == "adapter_id"));
        let session_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0);

        let mut task_stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
        let task_columns = task_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i32>(5)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            task_columns
                .iter()
                .any(|(name, primary_key)| name == "task_path" && *primary_key > 0)
        );

        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE task_id = 'task-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(task_count, 1);
        drop(stmt);
        drop(session_stmt);
        drop(task_stmt);
        drop(conn);
        let migrated_results = index.search_tasks("Task", 10).unwrap();
        assert_eq!(migrated_results.len(), 1);
        assert_eq!(migrated_results[0].task_id, "task-1");
    }

    #[test]
    fn task_index_identity_is_workspace_path_not_local_task_id() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let task_a = camino::Utf8PathBuf::from_path_buf(
            dir.path()
                .join("projects")
                .join("a")
                .join("tasks")
                .join("task-001"),
        )
        .unwrap();
        let task_b = camino::Utf8PathBuf::from_path_buf(
            dir.path()
                .join("projects")
                .join("b")
                .join("tasks")
                .join("task-001"),
        )
        .unwrap();
        for (task_dir, title) in [(&task_a, "Shared Alpha"), (&task_b, "Shared Beta")] {
            std::fs::create_dir_all(task_dir.join("authoring").as_std_path()).unwrap();
            crate::storage::write_json(
                &task_dir.join("task.json"),
                &TaskState {
                    version: crate::domain::VERSION.to_string(),
                    id: "task-001".to_string(),
                    title: Some(title.to_string()),
                    description: None,
                    uuid: None,
                },
            )
            .unwrap();
            std::fs::write(
                task_dir
                    .join("authoring")
                    .join("requirement.md")
                    .as_std_path(),
                "shared requirement",
            )
            .unwrap();
            index.index_task(task_dir, "task-001").unwrap();
        }

        let results = index.search_tasks("shared", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_ne!(results[0].task_path, results[1].task_path);

        index.delete_task(&task_a).unwrap();
        let remaining = index.search_tasks("shared", 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].task_path, task_b.as_str());
    }

    #[test]
    fn delete_run_removes_only_matching_sessions_and_prompts() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        {
            let conn = index.conn.lock().unwrap();
            for (attempt_path, task_id, run_id) in [
                (
                    "/tmp/workspace-a/tasks/task-1/runs/run-a/attempt-a",
                    "task-1",
                    "run-a",
                ),
                (
                    "/tmp/workspace-a/tasks/task-1/runs/run-b/attempt-b",
                    "task-1",
                    "run-b",
                ),
                (
                    "/tmp/workspace-b/tasks/task-1/runs/run-a/attempt-c",
                    "task-1",
                    "run-a",
                ),
                (
                    "/tmp/workspace-a/tasks/task-1/runs/run-a-extra/attempt-d",
                    "task-1",
                    "run-a-extra",
                ),
            ] {
                conn.execute(
                    "INSERT INTO sessions (
                         attempt_path, task_id, run_id, round_id, node_id, attempt_id
                     ) VALUES (?1, ?2, ?3, 'round-1', 'node-1', ?4)",
                    params![attempt_path, task_id, run_id, format!("attempt-{run_id}")],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO session_prompts (
                         id, attempt_path, timestamp, text, normalized_text
                     ) VALUES (?1, ?2, '', ?1, ?1)",
                    params![format!("prompt-{run_id}"), attempt_path],
                )
                .unwrap();
            }
        }

        index
            .delete_run(Utf8Path::new("/tmp/workspace-a/tasks/task-1/runs/run-a"))
            .unwrap();

        let conn = index.conn.lock().unwrap();
        let remaining_sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        let remaining_prompts: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_prompts", [], |row| row.get(0))
            .unwrap();
        let remaining_run: String = conn
            .query_row("SELECT run_id FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining_sessions, 3);
        assert_eq!(remaining_prompts, 3);
        assert_eq!(remaining_run, "run-b");
    }

    #[test]
    fn delete_run_query_plan_uses_attempt_path_primary_key_indexes() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let run_path = "/tmp/workspace/tasks/task-1/runs/run-a";
        let descendant_start = format!("{run_path}/");
        let descendant_end = format!("{run_path}0");
        let conn = index.conn.lock().unwrap();

        for (sql, expected_index) in [
            (DELETE_RUN_PROMPTS_SQL, "sqlite_autoindex_session_prompts_1"),
            (DELETE_RUN_SESSIONS_SQL, "sqlite_autoindex_sessions_1"),
        ] {
            let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let details = statement
                .query_map(params![run_path, descendant_start, descendant_end], |row| {
                    row.get::<_, String>(3)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                details.iter().any(|detail| detail.contains(expected_index)),
                "query plan did not use {expected_index}: {details:?}"
            );
            assert!(
                details.iter().all(|detail| !detail.contains("SCAN ")),
                "query plan performed a scan: {details:?}"
            );
        }
    }

    #[test]
    fn captured_normalized_run_identity_survives_alias_target_deletion() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let canonical_workspace = dir.path().join("canonical-workspace");
        let canonical_run = canonical_workspace
            .join("tasks")
            .join("task-1")
            .join("runs")
            .join("run-a");
        let canonical_attempt = canonical_run.join("attempt-1");
        std::fs::create_dir_all(&canonical_attempt).unwrap();
        let alias_workspace = dir.path().join("workspace-alias");

        #[cfg(windows)]
        if let Err(error) =
            std::os::windows::fs::symlink_dir(&canonical_workspace, &alias_workspace)
        {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                return;
            }
            panic!("failed to create directory symlink: {error}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&canonical_workspace, &alias_workspace).unwrap();

        let alias_run = camino::Utf8PathBuf::from_path_buf(
            alias_workspace
                .join("tasks")
                .join("task-1")
                .join("runs")
                .join("run-a"),
        )
        .unwrap();
        let canonical_attempt = camino::Utf8PathBuf::from_path_buf(canonical_attempt).unwrap();
        let captured_run_identity = crate::storage::normalize_workspace_path(&alias_run);
        let indexed_attempt_identity = crate::storage::normalize_workspace_path(&canonical_attempt);
        assert!(indexed_attempt_identity.starts_with(&format!("{captured_run_identity}/")));
        {
            let conn = index.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO sessions (
                     attempt_path, task_id, run_id, round_id, node_id, attempt_id
                 ) VALUES (?1, 'task-1', 'run-a', 'round-1', 'node-1', 'attempt-1')",
                params![indexed_attempt_identity],
            )
            .unwrap();
        }

        std::fs::remove_dir_all(&canonical_run).unwrap();
        index
            .delete_run_by_normalized_path(&captured_run_identity)
            .unwrap();

        let remaining: i64 = index
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn loaded_session_candidate_does_not_reappear_after_source_run_deletion() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let run_dir = camino::Utf8PathBuf::from_path_buf(dir.path().join("run-a")).unwrap();
        let attempt_dir = run_dir.join("attempt-1");
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        let snapshot_path = attempt_dir.join("acp.snapshot.json");
        crate::acp::events::begin_session_turn(
            &snapshot_path,
            &crate::acp::events::AcpPromptSubmission {
                turn_id: "turn-stale".to_string(),
                operation_id: "operation-stale".to_string(),
                adapter_id: "test".to_string(),
                adapter_display_name: "Test".to_string(),
                cwd: attempt_dir.to_string(),
                input: crate::provider::ConversationPromptInput {
                    display_text: "stale prompt".to_string(),
                    quotes: Vec::new(),
                },
                attachment_paths: Vec::new(),
                admitted_at: "2026-08-27T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        crate::acp::events::write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[crate::acp::events::user_prompt_event(
                1,
                "session-stale".to_string(),
                "stale prompt".to_string(),
                Some("prompt-stale".to_string()),
                false,
                Vec::new(),
            )],
        )
        .unwrap();
        let context = AttemptIndexContext {
            task_id: "task-1".to_string(),
            run_id: "run-a".to_string(),
            round_id: "round-1".to_string(),
            node_id: "node-1".to_string(),
            attempt_id: "attempt-1".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let candidate = load_session_index_candidate(&attempt_dir, &context);
        assert_eq!(candidate.prompts.len(), 1);

        std::fs::remove_dir_all(run_dir.as_std_path()).unwrap();
        index.commit_session_index_candidate(candidate).unwrap();

        let conn = index.conn.lock().unwrap();
        let sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        let prompts: i64 = conn
            .query_row("SELECT COUNT(*) FROM session_prompts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sessions, 0);
        assert_eq!(prompts, 0);
    }

    #[cfg(windows)]
    #[test]
    fn delete_run_uses_normalized_windows_identity() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        {
            let conn = index.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO sessions (
                     attempt_path, task_id, run_id, round_id, node_id, attempt_id
                 ) VALUES (?1, 'task-1', 'run-a', 'round-1', 'node-1', 'attempt-1')",
                params!["c:/workspace/tasks/task-1/runs/run-a/attempt-1"],
            )
            .unwrap();
        }

        index
            .delete_run(Utf8Path::new("c:/workspace/tasks/task-1/runs/run-a"))
            .unwrap();

        let remaining: i64 = index
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn scoped_task_search_filters_workspaces_before_applying_limit() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let projects_dir = camino::Utf8PathBuf::from_path_buf(dir.path().join("projects")).unwrap();
        let excluded_tasks = projects_dir.join("excluded").join("tasks");
        let included_tasks = projects_dir.join("included").join("tasks");

        for number in 1..=3 {
            let task_dir = excluded_tasks.join(format!("task-{number:03}"));
            std::fs::create_dir_all(task_dir.join("authoring").as_std_path()).unwrap();
            crate::storage::write_json(
                &task_dir.join("task.json"),
                &TaskState {
                    version: crate::domain::VERSION.to_string(),
                    id: format!("task-{number:03}"),
                    title: Some("Needle".to_string()),
                    description: None,
                    uuid: None,
                },
            )
            .unwrap();
            index
                .index_task(&task_dir, &format!("task-{number:03}"))
                .unwrap();
        }

        let included_task = included_tasks.join("task-001");
        std::fs::create_dir_all(included_task.join("authoring").as_std_path()).unwrap();
        crate::storage::write_json(
            &included_task.join("task.json"),
            &TaskState {
                version: crate::domain::VERSION.to_string(),
                id: "task-001".to_string(),
                title: Some("Needle in sidebar workspace".to_string()),
                description: None,
                uuid: None,
            },
        )
        .unwrap();
        index.index_task(&included_task, "task-001").unwrap();

        let results = index
            .search_tasks_in_task_roots("needle", &[included_tasks.to_string()], 1)
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_path, included_task.as_str());
    }

    #[test]
    fn task_search_supports_cjk_short_queries_and_mixed_script_substrings() {
        let dir = tempdir().unwrap();
        let db_path = camino::Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = SearchIndex::open(&db_path).unwrap();
        let tasks_dir = camino::Utf8PathBuf::from_path_buf(dir.path().join("tasks")).unwrap();
        let mixed_task = tasks_dir.join("task-001");
        std::fs::create_dir_all(mixed_task.join("authoring").as_std_path()).unwrap();
        crate::storage::write_json(
            &mixed_task.join("task.json"),
            &TaskState {
                version: crate::domain::VERSION.to_string(),
                id: "task-001".to_string(),
                title: Some("随便用askUserQuestion".to_string()),
                description: None,
                uuid: None,
            },
        )
        .unwrap();
        std::fs::write(
            mixed_task
                .join("authoring")
                .join("requirement.md")
                .as_std_path(),
            "随便用askUserQuestion工具问我几个问题",
        )
        .unwrap();
        index.index_task(&mixed_task, "task-001").unwrap();

        let hello_task = tasks_dir.join("task-002");
        std::fs::create_dir_all(hello_task.join("authoring").as_std_path()).unwrap();
        crate::storage::write_json(
            &hello_task.join("task.json"),
            &TaskState {
                version: crate::domain::VERSION.to_string(),
                id: "task-002".to_string(),
                title: Some("你好".to_string()),
                description: None,
                uuid: None,
            },
        )
        .unwrap();
        index.index_task(&hello_task, "task-002").unwrap();

        for query in ["随便", "askUser", "工具问"] {
            let results = index
                .search_tasks_in_task_roots(query, &[tasks_dir.to_string()], 10)
                .unwrap();
            assert_eq!(results.len(), 1, "query={query}");
            assert_eq!(results[0].task_path, mixed_task.as_str(), "query={query}");
            assert!(
                results[0]
                    .match_preview
                    .to_lowercase()
                    .contains(&query.to_lowercase()),
                "query={query}, preview={}",
                results[0].match_preview
            );
            let preview_lower = results[0].match_preview.to_lowercase();
            let query_lower = query.to_lowercase();
            let match_byte = preview_lower.find(&query_lower).unwrap();
            assert!(
                preview_lower[..match_byte].chars().count() <= 32,
                "query={query}, preview={}",
                results[0].match_preview
            );
        }

        let hello_results = index
            .search_tasks_in_task_roots("你好", &[tasks_dir.to_string()], 10)
            .unwrap();
        assert_eq!(hello_results.len(), 1);
        assert_eq!(hello_results[0].task_path, hello_task.as_str());
        assert_eq!(hello_results[0].match_preview, "你好");

        let issue_results = index
            .search_tasks_in_task_roots("问题", &[tasks_dir.to_string()], 10)
            .unwrap();
        assert_eq!(issue_results.len(), 1);
        assert_eq!(
            issue_results[0].match_preview,
            "随便用askUserQuestion工具问我几个问题"
        );
    }

    #[test]
    fn long_match_preview_keeps_the_keyword_near_the_front() {
        let text = format!("{}关键词{}", "前置内容".repeat(30), "后置内容".repeat(30));
        let preview = excerpt_around_search_term(&text, "关键词").unwrap();
        let match_byte = preview.find("关键词").unwrap();

        assert!(preview.starts_with('…'));
        assert!(preview[..match_byte].chars().count() <= 11);
        assert!(preview.ends_with('…'));
    }
}
