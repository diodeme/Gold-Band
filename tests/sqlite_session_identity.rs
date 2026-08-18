use camino::{Utf8Path, Utf8PathBuf};
use gold_band::storage::sqlite::{AttemptIndexContext, SearchIndex};
use gold_band::storage::{append_jsonl, write_json};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn write_session_snapshot(attempt_dir: &Utf8Path, session_id: Option<&str>) {
    write_json(
        &attempt_dir.join("acp.snapshot.json"),
        &json!({
            "adapterId": "npx",
            "adapterDisplayName": "Claude ACP",
            "cwd": attempt_dir.as_str(),
            "title": "Indexed session",
            "sessionId": session_id,
            "availability": if session_id.is_some() { "established" } else { "unavailable" },
            "latestTurnStatus": "completed",
            "restored": false,
            "stopReason": null,
            "capabilities": {},
            "createdAt": "2026-08-19T01:00:00Z",
            "updatedAt": "2026-08-19T01:01:00Z"
        }),
    )
    .unwrap();
}

#[test]
fn search_results_use_real_session_identity_without_copying_adapter_config() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
    let attempt_dir = Utf8PathBuf::from_path_buf(dir.path().join("attempt-001")).unwrap();
    std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
    let index = SearchIndex::open(&db_path).unwrap();
    let context = AttemptIndexContext {
        task_id: "task-001".to_string(),
        run_id: "run-001".to_string(),
        round_id: "round-001".to_string(),
        node_id: "develop".to_string(),
        attempt_id: "attempt-001".to_string(),
        outer_node_id: None,
        outer_attempt_id: None,
    };

    write_session_snapshot(&attempt_dir, Some("session-real-123"));
    append_jsonl(
        &attempt_dir.join("acp.timeline.jsonl"),
        &json!({
            "item": {
                "id": "prompt-event-1",
                "seq": 1,
                "timestamp": "2026-08-19T01:00:30Z",
                "kind": "userTextDelta",
                "sessionId": "session-real-123",
                "content": "Needle prompt",
                "raw": { "promptId": "prompt-1" }
            }
        }),
    )
    .unwrap();

    index.index_session_with_retry(&attempt_dir, &context);

    let sessions = index.search_sessions("Indexed", 10).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id.as_deref(), Some("session-real-123"));
    let prompts = index.search_prompts("Needle", 10).unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].session_id.as_deref(), Some("session-real-123"));

    write_session_snapshot(&attempt_dir, None);
    index.index_session_with_retry(&attempt_dir, &context);

    let sessions = index.search_sessions("Indexed", 10).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].session_id.is_none());
    assert!(serde_json::to_value(&sessions[0]).unwrap()["sessionId"].is_null());
    let prompts = index.search_prompts("Needle", 10).unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].session_id.is_none());
}

#[test]
fn schema_v4_removes_adapter_config_without_losing_tasks() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
    let conn = Connection::open(db_path.as_std_path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            task_id TEXT NOT NULL,
            task_path TEXT NOT NULL PRIMARY KEY,
            title TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            requirement_text TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT ''
        );
        INSERT INTO tasks (task_id, task_path, title)
        VALUES ('task-001', '/tmp/task-001', 'Preserved task');
        CREATE TABLE sessions (
            session_id TEXT,
            adapter_id TEXT NOT NULL DEFAULT '',
            attempt_path TEXT NOT NULL PRIMARY KEY
        );
        INSERT INTO sessions (session_id, adapter_id, attempt_path)
        VALUES ('session-real-123', 'npx', '/tmp/attempt-001');
        CREATE TABLE session_prompts (
            id TEXT NOT NULL PRIMARY KEY,
            session_id TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT ''
        );
        PRAGMA user_version = 4;",
    )
    .unwrap();
    drop(conn);

    let _index = SearchIndex::open(&db_path).unwrap();
    let conn = Connection::open(db_path.as_std_path()).unwrap();

    let schema_version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(schema_version, 5);
    let task_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(task_count, 1);
    let session_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(session_count, 0);

    let session_columns = conn
        .prepare("PRAGMA table_info(sessions)")
        .unwrap()
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
}
