use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use camino::Utf8Path;
use rusqlite::{Connection, params};
use tracing::warn;

use crate::acp::events::{AcpSessionMetadata, load_timeline_items};
use crate::runtime::TaskState;
use crate::storage::read_json;

// ── global singleton ─────────────────────────────────────────────────

static SEARCH_INDEX: OnceLock<Arc<SearchIndex>> = OnceLock::new();

pub fn init_search_index(db_path: &Utf8Path, projects_dir: &Utf8Path) -> Result<Arc<SearchIndex>, rusqlite::Error> {
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

// ── SearchIndex ──────────────────────────────────────────────────────

const MAX_RETRIES: u32 = 3;
const RETRY_DELAYS_MS: [u64; 3] = [200, 500, 1500];

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
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                task_id      TEXT NOT NULL PRIMARY KEY,
                task_path    TEXT NOT NULL,
                title        TEXT NOT NULL DEFAULT '',
                description  TEXT NOT NULL DEFAULT '',
                requirement_text TEXT NOT NULL DEFAULT '',
                created_at   TEXT NOT NULL DEFAULT '',
                updated_at   TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS sessions (
                session_id   TEXT NOT NULL,
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
                session_id    TEXT NOT NULL,
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
                USING fts5(title, description, requirement_text, content=tasks, content_rowid=rowid);

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
            END;"
        )?;
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
        let Ok(project_entries) = std::fs::read_dir(projects_dir.as_std_path()) else {
            return;
        };
        for project_entry in project_entries.flatten() {
            let Some(tasks_dir) = to_utf8(project_entry.path().join("tasks")) else {
                continue;
            };
            if !tasks_dir.is_dir() {
                continue;
            }
            let Ok(task_entries) = std::fs::read_dir(tasks_dir.as_std_path()) else {
                continue;
            };
            for task_entry in task_entries.flatten() {
                let Some(task_dir) = to_utf8(task_entry.path()) else {
                    continue;
                };
                if !task_dir.is_dir() {
                    continue;
                }
                let Some(task_id) = file_name(&task_dir) else {
                    continue;
                };
                let _ = self.index_task(&task_dir, task_id);
                self.backfill_task_attempts(&task_dir, task_id);
            }
        }
    }

    fn backfill_task_attempts(&self, task_dir: &Utf8Path, task_id: &str) {
        let runs_dir = task_dir.join("runs");
        let Ok(run_entries) = std::fs::read_dir(runs_dir.as_std_path()) else {
            return;
        };
        for run_entry in run_entries.flatten() {
            let Some(run_dir) = to_utf8(run_entry.path()) else { continue };
            if !run_dir.is_dir() { continue; }
            let Some(run_id) = file_name(&run_dir) else { continue };

            let rounds_dir = run_dir.join("rounds");
            let Ok(round_entries) = std::fs::read_dir(rounds_dir.as_std_path()) else {
                continue;
            };
            for round_entry in round_entries.flatten() {
                let Some(round_dir) = to_utf8(round_entry.path()) else { continue };
                if !round_dir.is_dir() { continue; }
                let Some(round_id) = file_name(&round_dir) else { continue };

                let nodes_dir = round_dir.join("nodes");
                let Ok(node_entries) = std::fs::read_dir(nodes_dir.as_std_path()) else {
                    continue;
                };
                for node_entry in node_entries.flatten() {
                    let Some(node_dir) = to_utf8(node_entry.path()) else { continue };
                    if !node_dir.is_dir() { continue; }
                    let Some(node_id) = file_name(&node_dir) else { continue };

                    let Ok(attempt_entries) = std::fs::read_dir(node_dir.as_std_path()) else {
                        continue;
                    };
                    for attempt_entry in attempt_entries.flatten() {
                        let Some(attempt_dir) = to_utf8(attempt_entry.path()) else {
                            continue;
                        };
                        if !attempt_dir.is_dir() { continue; }
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
                        let _ = self.index_session(&attempt_dir, &ctx);
                    }
                }
            }
        }
    }

    // ── index with retry ────────────────────────────────────────

    /// Index a session attempt. Each retry re-reads `acp.snapshot.json` and
    /// `acp.timeline.jsonl` fresh from disk, so the write always uses the
    /// latest state even if the session was still streaming during earlier
    /// attempts.
    pub fn index_session_with_retry(
        &self,
        attempt_dir: &Utf8Path,
        ctx: &AttemptIndexContext,
    ) {
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
        let snapshot = read_snapshot(attempt_dir);
        let conn = self.conn.lock().expect("search index lock poisoned");
        let tx = conn.unchecked_transaction()?;

        let attempt_path = attempt_dir.to_string();
        let (session_id, status, title, created_at, updated_at) = snapshot
            .as_ref()
            .map(|s| {
                (
                    s.adapter_id.as_str(),
                    s.status.as_str(),
                    s.title.as_deref().unwrap_or(""),
                    s.created_at.as_str(),
                    s.updated_at.as_str(),
                )
            })
            .unwrap_or(("", "", "", "", ""));

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
                ctx.task_id,
                ctx.run_id,
                ctx.round_id,
                ctx.node_id,
                ctx.attempt_id,
                ctx.outer_node_id,
                ctx.outer_attempt_id,
                title,
                status,
                created_at,
                updated_at,
            ],
        )?;

        let timeline = load_timeline_items(&attempt_dir.join("acp.timeline.jsonl"))
            .unwrap_or_default();
        for item in &timeline {
            if item.kind != "userTextDelta" {
                continue;
            }
            let Some(content) = &item.content else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let prompt_id = item
                .raw
                .as_ref()
                .and_then(|r| r.get("promptId"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let normalized = normalize_for_search(content);
            tx.execute(
                "INSERT INTO session_prompts
                    (id, attempt_path, session_id, prompt_id, timestamp, text, normalized_text)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(attempt_path, id) DO UPDATE SET
                    text=excluded.text,
                    normalized_text=excluded.normalized_text",
                params![item.id, attempt_path, session_id, prompt_id, item.timestamp, content, normalized],
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
             LIMIT ?2"
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

    fn index_task(
        &self,
        task_dir: &Utf8Path,
        task_id: &str,
    ) -> Result<(), rusqlite::Error> {
        let task_path = task_dir.to_string();
        let task: Option<TaskState> = read_json(&task_dir.join("task.json")).ok();
        let requirement_text = std::fs::read_to_string(
            task_dir.join("authoring").join("requirement.md").as_std_path(),
        )
        .unwrap_or_default();

        let (title, description, created_at, updated_at) = task
            .as_ref()
            .map(|t| {
                (
                    t.title.as_deref().unwrap_or(""),
                    t.description.as_deref().unwrap_or(""),
                    "",  // TaskState has no created_at; snapshot-based timestamps don't apply here
                    "",  // We could derive from file mtime, but keep it simple
                )
            })
            .unwrap_or(("", "", "", ""));

        let conn = self.conn.lock().expect("search index lock poisoned");
        conn.execute(
            "INSERT INTO tasks (task_id, task_path, title, description, requirement_text, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(task_id) DO UPDATE SET
                title=excluded.title,
                description=excluded.description,
                requirement_text=excluded.requirement_text,
                updated_at=excluded.updated_at",
            params![task_id, task_path, title, description, requirement_text, created_at, updated_at],
        )?;
        Ok(())
    }

    // ── task search ────────────────────────────────────────────

    pub fn search_tasks(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<TaskSearchResult>, rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        let normalized = normalize_for_search(query);
        let mut stmt = conn.prepare(
            "SELECT t.task_id, t.task_path, t.title, t.description,
                    substr(t.requirement_text, 1, 500)
             FROM tasks_fts fts
             JOIN tasks t ON fts.rowid = t.rowid
             WHERE tasks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![normalized, limit as i64], |row| {
            Ok(TaskSearchResult {
                task_id: row.get(0)?,
                task_path: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                requirement_preview: row.get(4)?,
            })
        })?;
        rows.collect()
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
             LIMIT ?2"
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

fn read_snapshot(attempt_dir: &Utf8Path) -> Option<AcpSessionMetadata> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    if snapshot_path.exists() {
        return read_json(&snapshot_path).ok();
    }
    let session_path = attempt_dir.join("acp.session.json");
    if session_path.exists() {
        return read_json(&session_path).ok();
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

// ── search result types ─────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptSearchResult {
    pub prompt_event_id: String,
    pub session_id: String,
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
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchResult {
    pub session_id: String,
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
