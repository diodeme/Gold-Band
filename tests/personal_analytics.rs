use std::fs;
use std::path::Path;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{Duration, Local, TimeZone};
use gold_band::personal_analytics::{
    AnalyticsInsightConfidence, AnalyticsInsightSection, PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION,
    PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS, PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS,
    PersonalAnalyticsInsight, PersonalAnalyticsNarrative, build_personal_analytics_projection,
    index::PersonalAnalyticsDateRange, index::PersonalAnalyticsIndex,
    personal_analytics_narrative_schema,
};
use gold_band::prompts::{
    PERSONAL_ANALYTICS_SYSTEM_EN, PERSONAL_ANALYTICS_SYSTEM_ZH_CN, PERSONAL_ANALYTICS_USER_EN,
    PERSONAL_ANALYTICS_USER_ZH_CN, render,
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("test path has a parent")).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn mode_specific_metrics_exclude_raw_frames() {
    let root = tempdir().unwrap();
    let project = root.path().join("project-a");
    write(
        &project.join("project.json"),
        r#"{"version":"0.1","projectId":"project-a"}"#,
    );
    for (task, mode, outcome) in [
        ("task-direct", "direct", "success"),
        ("task-workflow", "workflow", "success"),
        ("task-auto", "auto", "failure"),
    ] {
        write(
            &project.join(format!("tasks/{task}/authoring/task.json")),
            &format!(r#"{{"version":"0.1","id":"{task}"}}"#),
        );
        write(
            &project.join(format!("tasks/{task}/authoring/conversation.json")),
            &format!(r#"{{"version":"0.1","runMode":"{mode}"}}"#),
        );
        write(
            &project.join(format!("tasks/{task}/runs/run-001/run.json")),
            &format!(
                r#"{{"version":"0.1","status":"completed","outcome":"{outcome}","started_at":"2026-08-17T10:00:00Z","updated_at":"2026-08-17T10:01:00Z"}}"#
            ),
        );
    }
    write(
        &project.join("tasks/task-direct/runs/run-001/rounds/round-001/nodes/direct-agent/attempt-001/acp.prompt-usage.jsonl"),
        concat!(
            r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-17T10:00:01Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-17T10:00:05Z","usage":{"inputTokens":10,"outputTokens":5,"totalTokens":15}}"#,
            "\n",
            r#"{"kind":"promptStarted","turn_id":"turn-2","turn_seq":2,"timestamp":"2026-08-17T10:00:06Z"}"#,
        ),
    );
    write(
        &project.join("tasks/task-direct/runs/run-001/acp.raw.jsonl"),
        "not-json-and-must-never-be-read",
    );

    let output = build_personal_analytics_projection(
        Utf8Path::from_path(root.path()).unwrap(),
        "watermark".to_string(),
        |_, _| {},
        || false,
    )
    .unwrap();

    assert_eq!(
        output
            .projection
            .reliability
            .direct_reply_completion_rate
            .denominator,
        2
    );
    assert_eq!(
        output
            .projection
            .reliability
            .direct_reply_completion_rate
            .numerator,
        1
    );
    assert_eq!(
        output
            .projection
            .reliability
            .workflow_run_terminal_success_rate
            .numerator,
        1
    );
    assert_eq!(
        output
            .projection
            .reliability
            .auto_outer_run_terminal_success_rate
            .numerator,
        0
    );
    assert_eq!(output.projection.source_coverage.corrupt_files, 0);
    assert!(output.projection.source_coverage.skipped_files >= 1);
    assert_eq!(output.projection.recent_tasks.len(), 2);
    assert!(
        output
            .projection
            .recent_tasks
            .iter()
            .all(|task| task.mode != "direct")
    );
}

#[test]
fn corrupt_files_are_isolated_and_semantic_content_is_bounded() {
    let root = tempdir().unwrap();
    let project = root.path().join("project-a/tasks/task-1");
    write(&project.join("authoring/task.json"), "{broken");
    write(
        &project.join("authoring/requirement.md"),
        &format!("{}你{}", "x".repeat(4_799), "y".repeat(500)),
    );

    let output = build_personal_analytics_projection(
        Utf8Path::from_path(root.path()).unwrap(),
        "watermark".to_string(),
        |_, _| {},
        || false,
    )
    .unwrap();

    assert_eq!(output.projection.source_coverage.corrupt_files, 1);
    assert_eq!(output.semantic_batch.items.len(), 1);
    assert_eq!(
        output.semantic_batch.items[0].content.chars().count(),
        PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let (report, semantic_items) = index
        .report_with_semantic_batch(&PersonalAnalyticsDateRange::default(), "semantic".into())
        .unwrap();
    assert_eq!(report.source_coverage.corrupt_files, 1);
    assert_eq!(semantic_items.len(), 1);
    assert_eq!(
        semantic_items[0].content.chars().count(),
        PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS
    );
    assert!(personal_analytics_narrative_schema().is_object());
}

#[test]
fn active_durations_use_session_timing_and_zero_fill_missing_snapshots() {
    let root = tempdir().unwrap();
    let project = root.path().join("project-a");
    for (task, updated_at) in [
        ("task-session", "1786868389Z"),
        ("task-missing", "1780976400Z"),
    ] {
        write(
            &project.join(format!("tasks/{task}/authoring/task.json")),
            &format!(r#"{{"version":"0.1","id":"{task}","title":"{task}"}}"#),
        );
        write(
            &project.join(format!("tasks/{task}/authoring/conversation.json")),
            r#"{"version":"2","runMode":"workflow"}"#,
        );
        write(
            &project.join(format!("tasks/{task}/runs/run-001/run.json")),
            &format!(
                r#"{{"version":"0.1","status":"completed","outcome":"success","started_at":"1780976340Z","updated_at":"{updated_at}"}}"#
            ),
        );
        write(
            &project.join(format!(
                "tasks/{task}/runs/run-001/rounds/round-001/nodes/dev/attempt-001/node.json"
            )),
            r#"{"version":"0.1","node_id":"dev","status":"completed","outcome":"success","started_at":"1780976340Z","finished_at":"1786868389Z"}"#,
        );
    }
    write(
        &project.join("tasks/task-session/runs/run-001/rounds/round-001/nodes/dev/attempt-001/acp.snapshot.json"),
        r#"{"version":"0.1","timing":{"sessionElapsedSeconds":60,"paused":true}}"#,
    );

    let output = build_personal_analytics_projection(
        Utf8Path::from_path(root.path()).unwrap(),
        "watermark".to_string(),
        |_, _| {},
        || false,
    )
    .unwrap();

    assert_eq!(output.projection.efficiency.terminal_run_sample_count, 2);
    assert_eq!(
        output
            .projection
            .efficiency
            .observed_terminal_run_active_seconds,
        60
    );
    assert_eq!(
        output
            .projection
            .efficiency
            .average_terminal_run_active_seconds,
        Some(30.0)
    );
    assert_eq!(
        output
            .projection
            .efficiency
            .active_duration_zero_filled_count,
        1
    );
    assert_eq!(output.projection.recent_tasks.len(), 2);
    let session_task = output
        .projection
        .recent_tasks
        .iter()
        .find(|task| task.title == "task-session")
        .unwrap();
    assert_eq!(session_task.active_duration_seconds, 60);
    assert!(!session_task.active_duration_zero_filled);
    let node = &output.projection.efficiency.node_aggregates[0];
    assert_eq!(node.total_active_duration_seconds, 60);
    assert_eq!(node.average_active_duration_seconds, 30.0);
    assert_eq!(node.active_duration_zero_filled_count, 1);
}

#[test]
fn bilingual_prompts_render_with_attachment_only_boundary() {
    for system in [
        PERSONAL_ANALYTICS_SYSTEM_ZH_CN,
        PERSONAL_ANALYTICS_SYSTEM_EN,
    ] {
        let rendered = render(system, json!({ "report_schema": { "type": "object" } })).unwrap();
        assert!(!rendered.contains("{{"));
        assert!(rendered.contains("acp.raw.jsonl"));
    }
    for user in [PERSONAL_ANALYTICS_USER_ZH_CN, PERSONAL_ANALYTICS_USER_EN] {
        let rendered = render(
            user,
            json!({
                "operation_id": "operation-1",
                "report_schema_version": "2.1.0",
                "source_watermark": "watermark",
                "index_revision": 2,
                "date_range": "{\"start\":null,\"end\":null}",
                "projection_path": "projection.json",
                "content_manifest_path": "content-manifest.json",
                "semantic_batch_manifest_path": "semantic-batch.json",
                "coverage_summary": "{}",
            }),
        )
        .unwrap();
        assert!(!rendered.contains("{{"));
        assert!(
            rendered.contains("不允许据此读取原始文件")
                || rendered.contains("do not grant permission to read original files")
        );
    }
}

#[test]
fn sqlite_index_syncs_reopens_incrementally_and_queries_date_ranges() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-1");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","title":"Indexed task"}"#,
    );
    write(
        &task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    let run = task.join("runs/run-1");
    write(
        &run.join("run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    let attempt = run.join("attempt-1");
    write(
        &attempt.join("node.json"),
        r#"{"version":"1.0","node_id":"plan","resolved_config":{"provider":"agent-a"}}"#,
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
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z","usage":{"inputTokens":100,"outputTokens":50,"totalTokens":150}}"#,
        ),
    );
    write(
        &attempt.join("acp.timeline.jsonl"),
        r#"{"item":{"kind":"toolCall","raw":{"name":"read_file"}}}"#,
    );
    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let first = index.sync(&projects, |_, _| {}, || false).unwrap();
    assert_eq!(first.reparsed_files, 7);
    let all = index
        .report(&PersonalAnalyticsDateRange::default(), "all".into())
        .unwrap();
    assert_eq!(
        all.reliability.workflow_run_terminal_success_rate.numerator,
        1
    );
    assert_eq!(all.efficiency.observed_terminal_run_active_seconds, 60);
    assert_eq!(all.token_usage.total_tokens, 150);
    assert_eq!(all.overview.conversation_count, 1);
    assert_eq!(all.index_revision, first.index_revision);
    drop(index);
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let reopened_state = index.state().unwrap();
    assert_eq!(reopened_state.schema_version, 9);
    assert_eq!(reopened_state.index_revision, first.index_revision);
    let reopened = index
        .report(&PersonalAnalyticsDateRange::default(), "reopened".into())
        .unwrap();
    assert_eq!(reopened.overview.task_count, 1);
    assert_eq!(reopened.token_usage.total_tokens, 150);
    assert_eq!(reopened.index_revision, first.index_revision);
    let january = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-01-01".into()),
                end: Some("2026-01-31".into()),
            },
            "january".into(),
        )
        .unwrap();
    assert_eq!(
        january
            .reliability
            .workflow_run_terminal_success_rate
            .denominator,
        0
    );
    assert_eq!(january.token_usage.total_tokens, 0);
    let unchanged = index.sync(&projects, |_, _| {}, || false).unwrap();
    assert_eq!(unchanged.reparsed_files, 0);
    assert_eq!(unchanged.index_revision, first.index_revision);

    write(
        &root.path().join("project-a/project.json"),
        r#"{"version":"2.0","name":"Future project"}"#,
    );
    write(&run.join("attempt-2/node.json"), "{ not json");
    let degraded = index.sync(&projects, |_, _| {}, || false).unwrap();
    assert_eq!(degraded.reparsed_files, 2);
    assert!(degraded.index_revision > first.index_revision);
    let connection = Connection::open(db.as_std_path()).unwrap();
    let mut statuses = connection
        .prepare("SELECT sourcePath, parseStatus FROM analytics_sources WHERE parseStatus IN ('corrupt', 'unknown-version') ORDER BY sourcePath")
        .unwrap();
    let degraded_sources = statuses
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        degraded_sources,
        vec![
            (
                "project-a/project.json".to_string(),
                "unknown-version".to_string()
            ),
            (
                "project-a/tasks/task-1/runs/run-1/attempt-2/node.json".to_string(),
                "corrupt".to_string()
            ),
        ]
    );
    let project_task_count: i64 = connection
        .query_row(
            "SELECT taskCount FROM analytics_projects WHERE projectLocator = 'project-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(project_task_count, 1);
    let usage_total: i64 = connection
        .query_row(
            "SELECT totalTokens FROM analytics_usage WHERE taskLocator = 'project-a/task-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(usage_total, 150);
    let mut counter_rows = connection
        .prepare("SELECT kind, name, count FROM analytics_event_counts ORDER BY kind, name")
        .unwrap();
    let counters = counter_rows
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        counters,
        vec![("tool".to_string(), "read_file".to_string(), 1)]
    );
}

#[test]
fn sqlite_index_does_not_admit_managed_worktree_checkout_files() {
    let root = tempdir().unwrap();
    let canonical_task = root.path().join("project-a/tasks/task-real");
    write(
        &canonical_task.join("task.json"),
        r#"{"version":"1.0","title":"Canonical task"}"#,
    );
    write(
        &canonical_task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    write(
        &canonical_task.join("runs/run-1/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-30T00:00:00Z"}"#,
    );
    write(
        &canonical_task.join("attachments/notes.txt"),
        "not an analytics source",
    );

    let checkout_task = root
        .path()
        .join("project-a/worktrees/run-worktree/tasks/task-shadow");
    write(
        &checkout_task.join("task.json"),
        r#"{"version":"1.0","title":"Checkout shadow"}"#,
    );
    write(
        &checkout_task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    write(
        &checkout_task.join("runs/run-shadow/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"failure","updated_at":"2026-08-30T00:00:00Z"}"#,
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let stats = index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "worktree-scope".into(),
        )
        .unwrap();

    assert_eq!(stats.reparsed_files, 3);
    assert_eq!(report.overview.task_count, 1);
    assert_eq!(report.overview.run_count, 1);
    let indexed_worktree_sources: i64 = Connection::open(db.as_std_path())
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM analytics_sources WHERE sourcePath LIKE '%/worktrees/%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(indexed_worktree_sources, 0);
    let indexed_sources: i64 = Connection::open(db.as_std_path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM analytics_sources", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(indexed_sources, 3);
}

#[test]
fn bounded_task_activity_uses_only_facts_inside_the_requested_range() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-1");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","title":"Scoped activity"}"#,
    );
    write(
        &task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow","lastActivityAt":"2026-08-20T12:00:00Z"}"#,
    );
    let in_range_run = task.join("runs/run-1");
    write(
        &in_range_run.join("run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    let attempt = in_range_run.join("attempt-1");
    write(
        &attempt.join("node.json"),
        r#"{"version":"1.0","node_id":"plan","resolved_config":{"provider":"agent-a"}}"#,
    );
    write(
        &attempt.join("acp.prompt-usage.jsonl"),
        concat!(
            r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:01Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z","usage":{"totalTokens":10}}"#,
        ),
    );
    write(
        &task.join("runs/run-2/run.json"),
        r#"{"version":"1.0","status":"failed","outcome":"failure","updated_at":"2026-08-20T13:00:00Z"}"#,
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-18".into()),
                end: Some("2026-08-18".into()),
            },
            "scoped-activity".into(),
        )
        .unwrap();

    let task = report
        .recent_tasks
        .first()
        .expect("task has in-range activity");
    assert_eq!(task.latest_run_id.as_deref(), Some("run-1"));
    assert_eq!(task.status, "completed");
    assert_eq!(
        task.last_activity_at.as_deref(),
        Some("2026-08-18T02:00:02Z")
    );
}

#[test]
fn sqlite_direct_metrics_use_current_run_and_prompt_journal_model() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-direct");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","id":"task-direct","title":"Current direct task"}"#,
    );
    write(
        &task.join("authoring/conversation.json"),
        r#"{"version":"3","runMode":"direct","createdAt":"2026-08-18T01:00:00Z"}"#,
    );
    let run = task.join("runs/run-001");
    write(
        &run.join("run.json"),
        r#"{"version":"0.1","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    let attempt = run.join("rounds/round-001/nodes/direct-agent/attempt-001");
    write(
        &attempt.join("node.json"),
        r#"{"version":"0.1","node_id":"direct-agent","status":"completed","outcome":"success","resolved_config":{"provider":"claude-acp"}}"#,
    );
    write(
        &attempt.join("acp.snapshot.json"),
        r#"{"version":"0.1","timing":{"sessionElapsedSeconds":30}}"#,
    );
    write(
        &attempt.join("acp.prompt-usage.jsonl"),
        concat!(
            r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:01Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z","usage":{"inputTokens":100,"outputTokens":50,"totalTokens":150}}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-2","turn_seq":2,"timestamp":"2026-08-18T02:00:03Z","usage":{"inputTokens":30,"outputTokens":20,"totalTokens":50}}"#,
            "\n",
            r#"{"kind":"promptStarted","turn_id":"turn-3","turn_seq":3,"timestamp":"2026-08-18T02:00:04Z"}"#,
        ),
    );
    write(
        &task.join("turns/legacy-turn/turn.json"),
        r#"{"record":{"data":{"status":"completed"}}}"#,
    );
    write(
        &task.join("acp.prompt-usage.jsonl"),
        r#"{"usage":{"inputTokens":9000,"outputTokens":1000,"totalTokens":10000}}"#,
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "direct-current".into(),
        )
        .unwrap();

    assert_eq!(report.overview.run_count, 1);
    assert_eq!(report.overview.turn_count, 3);
    assert_eq!(
        report.reliability.direct_reply_completion_rate.denominator,
        3
    );
    assert_eq!(report.reliability.direct_reply_completion_rate.numerator, 2);
    assert_eq!(
        report
            .reliability
            .direct_reply_completion_rate
            .unknown_count,
        1
    );
    assert_eq!(
        report
            .reliability
            .workflow_run_terminal_success_rate
            .denominator,
        0
    );
    assert_eq!(report.token_usage.total_tokens, 200);
    assert!(report.recent_tasks.is_empty());
    assert_eq!(report.token_usage.top_token_tasks[0].mode, "direct");
    assert_eq!(
        report.token_usage.top_token_tasks[0].latest_run_id,
        Some("run-001".to_string())
    );
    let unit_type: String = Connection::open(db.as_std_path())
        .unwrap()
        .query_row(
            "SELECT unitType FROM analytics_runs WHERE taskLocator='project-a/task-direct'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(unit_type, "direct-session");
    let legacy_source: (String, String) = Connection::open(db.as_std_path())
        .unwrap()
        .query_row(
            "SELECT sourceType, parseStatus FROM analytics_sources
             WHERE sourcePath='project-a/tasks/task-direct/acp.prompt-usage.jsonl'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(legacy_source, ("other".to_string(), "skipped".to_string()));
}

#[test]
fn sqlite_direct_started_only_reply_is_scoped_by_its_own_date() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-direct");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","id":"task-direct","title":"Cross-date direct task"}"#,
    );
    write(
        &task.join("authoring/conversation.json"),
        r#"{"version":"3","runMode":"direct"}"#,
    );
    write(
        &task.join("runs/run-001/run.json"),
        r#"{"version":"0.1","status":"completed","outcome":"success","updated_at":"2026-08-17T02:00:00Z"}"#,
    );
    write(
        &task.join(
            "runs/run-001/rounds/round-001/nodes/direct-agent/attempt-001/acp.prompt-usage.jsonl",
        ),
        r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:00Z"}"#,
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-18".into()),
                end: Some("2026-08-18".into()),
            },
            "direct-started-only".into(),
        )
        .unwrap();

    assert_eq!(report.overview.task_count, 1);
    assert_eq!(report.overview.run_count, 0);
    assert_eq!(report.overview.turn_count, 1);
    assert_eq!(
        report.reliability.direct_reply_completion_rate.denominator,
        1
    );
    assert_eq!(report.reliability.direct_reply_completion_rate.numerator, 0);
    assert_eq!(
        report
            .reliability
            .direct_reply_completion_rate
            .unknown_count,
        1
    );
}

#[test]
fn sqlite_cross_date_tokens_use_navigation_run_without_changing_range_metrics() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-workflow");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","id":"task-workflow","title":"Cross-date workflow task"}"#,
    );
    write(
        &task.join("authoring/conversation.json"),
        r#"{"version":"3","runMode":"workflow"}"#,
    );
    write(
        &task.join("runs/run-001/run.json"),
        r#"{"version":"0.1","status":"completed","outcome":"success","updated_at":"2026-08-19T02:00:00Z"}"#,
    );
    write(
        &task.join("runs/run-001/rounds/round-001/nodes/dev/attempt-001/acp.prompt-usage.jsonl"),
        r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:00Z","usage":{"inputTokens":200,"outputTokens":100,"totalTokens":300}}"#,
    );

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-18".into()),
                end: Some("2026-08-18".into()),
            },
            "cross-date-tokens".into(),
        )
        .unwrap();

    assert_eq!(report.overview.run_count, 0);
    assert_eq!(
        report
            .reliability
            .workflow_run_terminal_success_rate
            .denominator,
        0
    );
    assert_eq!(report.token_usage.total_tokens, 300);
    assert_eq!(report.token_usage.top_token_tasks.len(), 1);
    assert_eq!(
        report.token_usage.top_token_tasks[0].latest_run_id,
        Some("run-001".to_string())
    );
    assert!(report.recent_tasks.is_empty());
}

#[test]
fn sqlite_counter_ranges_follow_each_event_timestamp_and_latest_revision() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-1");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","title":"Cross-date counters"}"#,
    );
    write(
        &task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    let first_day = Local
        .with_ymd_and_hms(2026, 8, 18, 12, 0, 0)
        .single()
        .unwrap();
    let second_day = first_day + Duration::days(1);
    write(
        &task.join("runs/run-1/run.json"),
        &format!(
            r#"{{"version":"1.0","status":"completed","outcome":"success","updated_at":"{}"}}"#,
            second_day.to_rfc3339()
        ),
    );
    let timeline = [
        json!({
            "patchType": "timelinePatch",
            "itemId": "tool-read",
            "revision": 1,
            "op": "upsert",
            "item": {
                "id": "tool-read",
                "seq": 1,
                "timestamp": first_day.to_rfc3339(),
                "kind": "toolCall",
                "status": "completed",
                "raw": { "name": "read_file" }
            }
        }),
        json!({
            "patchType": "timelinePatch",
            "itemId": "tool-write",
            "revision": 1,
            "op": "upsert",
            "item": {
                "id": "tool-write",
                "seq": 2,
                "timestamp": second_day.to_rfc3339(),
                "kind": "toolCall",
                "status": "processing",
                "raw": { "name": "write_file" }
            }
        }),
        json!({
            "patchType": "timelinePatch",
            "itemId": "tool-write",
            "revision": 2,
            "op": "upsert",
            "item": {
                "id": "tool-write",
                "seq": 2,
                "timestamp": second_day.to_rfc3339(),
                "kind": "toolCall",
                "status": "completed",
                "raw": { "name": "write_file" }
            }
        }),
    ]
    .into_iter()
    .map(|record| record.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    write(&task.join("acp.timeline.jsonl"), &timeline);

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();

    let second_day_report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-19".into()),
                end: Some("2026-08-19".into()),
            },
            "second-day".into(),
        )
        .unwrap();
    assert_eq!(second_day_report.context_and_tools.tool_call_count, 1);
    assert_eq!(second_day_report.context_and_tools.top_tools.len(), 1);
    assert_eq!(
        second_day_report.context_and_tools.top_tools[0].name,
        "write_file"
    );
    assert_eq!(second_day_report.context_and_tools.top_tools[0].count, 1);
}

#[test]
fn sqlite_semantic_batch_reports_full_eligibility_and_enforces_character_budget() {
    let root = tempdir().unwrap();
    for task_number in 0..125 {
        write(
            &root.path().join(format!(
                "project-a/tasks/task-{task_number:03}/authoring/requirement.md"
            )),
            &"x".repeat(PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS),
        );
    }
    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();

    let (report, semantic_items) = index
        .report_with_semantic_batch(&PersonalAnalyticsDateRange::default(), "semantic".into())
        .unwrap();
    let sampled_chars = semantic_items
        .iter()
        .map(|item| item.content.chars().count())
        .sum::<usize>();

    assert_eq!(report.source_coverage.semantic_eligible_items, 125);
    assert_eq!(report.source_coverage.semantic_sampled_items, 60);
    assert_eq!(semantic_items.len(), 60);
    assert!(sampled_chars <= PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS);
    assert_eq!(
        semantic_items.len() as u64,
        report.source_coverage.semantic_sampled_items
    );
}

#[test]
fn sqlite_excludes_auto_child_runs_and_preserves_retry_outcomes() {
    let root = tempdir().unwrap();
    let auto_task = root.path().join("project-a/tasks/task-auto");
    write(
        &auto_task.join("authoring/task.json"),
        r#"{"version":"1.0","title":"AUTO task"}"#,
    );
    write(
        &auto_task.join("authoring/conversation.json"),
        r#"{"version":"1.0","runMode":"auto"}"#,
    );
    write(
        &auto_task.join("runs/run-outer/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"failure","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    write(
        &auto_task.join("runs/run-child/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T03:00:00Z"}"#,
    );
    write(
        &auto_task.join(
            "runs/run-outer/rounds/round-1/nodes/ai-dynamic/attempt-1/dynamic/nodes/invoke-child/node.json",
        ),
        r#"{"version":"1.0","id":"invoke-child","kind":"workflow-invocation","outcome":"success","childRunId":"run-child"}"#,
    );

    let retry_task = root.path().join("project-a/tasks/task-retry");
    write(
        &retry_task.join("authoring/task.json"),
        r#"{"version":"1.0","title":"Retry task"}"#,
    );
    write(
        &retry_task.join("authoring/conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    write(
        &retry_task.join("runs/run-1/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T04:00:00Z"}"#,
    );
    for (attempt, outcome) in [("attempt-1", "failure"), ("attempt-2", "success")] {
        write(
            &retry_task.join(format!(
                "runs/run-1/rounds/round-1/nodes/plan/{attempt}/node.json"
            )),
            &format!(r#"{{"version":"1.0","node_id":"plan","outcome":"{outcome}"}}"#),
        );
    }

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "run-identity".into(),
        )
        .unwrap();

    assert_eq!(
        report
            .reliability
            .auto_outer_run_terminal_success_rate
            .denominator,
        1
    );
    assert_eq!(
        report
            .reliability
            .auto_outer_run_terminal_success_rate
            .numerator,
        0
    );
    assert_eq!(report.quality.retry_reentry_rate.numerator, 1);
    assert_eq!(report.quality.recovered_after_retry_count, 1);
    let auto_summary = report
        .recent_tasks
        .iter()
        .find(|task| task.task_id.as_deref() == Some("task-auto"))
        .unwrap();
    assert_eq!(auto_summary.latest_run_id.as_deref(), Some("run-outer"));
    assert_eq!(auto_summary.outcome.as_deref(), Some("failure"));

    let child_type: String = Connection::open(db.as_std_path())
        .unwrap()
        .query_row(
            "SELECT unitType FROM analytics_runs WHERE runLocator LIKE '%/runs/run-child'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(child_type, "auto-child-run");
}

#[test]
fn auto_dynamic_leaf_sources_share_one_canonical_attempt_identity() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-auto");
    write(
        &task.join("authoring/task.json"),
        r#"{"version":"1.0","title":"Dynamic identity"}"#,
    );
    write(
        &task.join("authoring/conversation.json"),
        r#"{"version":"1.0","runMode":"auto"}"#,
    );
    write(
        &task.join("runs/run-1/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    let dynamic_nodes =
        task.join("runs/run-1/rounds/round-1/nodes/ai-dynamic/attempt-1/dynamic/nodes");
    for (leaf, provider, seconds, tokens) in [
        ("worker-a", "agent-a", 11, 15),
        ("worker-b", "agent-b", 13, 25),
    ] {
        let leaf_root = dynamic_nodes.join(leaf);
        write(
            &leaf_root.join("node.json"),
            &format!(
                r#"{{"version":"1.0","id":"{leaf}","provider":"{provider}","outcome":"success"}}"#
            ),
        );
        write(
            &leaf_root.join("attempt-001/acp.snapshot.json"),
            &format!(r#"{{"version":"1.0","timing":{{"sessionElapsedSeconds":{seconds}}}}}"#),
        );
        write(
            &leaf_root.join("attempt-001/acp.prompt-usage.jsonl"),
            &format!(
                r#"{{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:01Z","usage":{{"inputTokens":{tokens},"totalTokens":{tokens}}}}}"#
            ),
        );
    }

    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let projection = build_personal_analytics_projection(
        &projects,
        "dynamic-projection".into(),
        |_, _| {},
        || false,
    )
    .unwrap()
    .projection;
    assert_eq!(projection.overview.attempt_count, 2);
    assert_eq!(projection.quality.retry_reentry_rate.numerator, 0);
    assert_eq!(
        projection.efficiency.observed_terminal_run_active_seconds,
        24
    );
    assert_eq!(projection.token_usage.total_tokens, 40);

    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let connection = Connection::open(db.as_std_path()).unwrap();
    let row_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM analytics_attempts
             WHERE attemptLocator LIKE '%/dynamic/nodes/%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row_count, 2);
    let worker_a = connection
        .query_row(
            "SELECT attemptLocator, nodeSourcePath, snapshotSourcePath, usageSourcePath,
                    nodeId, agent, outcome, sessionElapsedSeconds, totalTokens
             FROM analytics_attempts WHERE attemptLocator LIKE '%/dynamic/nodes/worker-a'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .unwrap();
    assert!(worker_a.0.ends_with("/dynamic/nodes/worker-a"));
    assert!(worker_a.1.is_some());
    assert!(worker_a.2.is_some());
    assert!(worker_a.3.is_some());
    assert_eq!(worker_a.4, "worker-a");
    assert_eq!(worker_a.5.as_deref(), Some("agent-a"));
    assert_eq!(worker_a.6.as_deref(), Some("success"));
    assert_eq!(worker_a.7, Some(11));
    assert_eq!(worker_a.8, 15);

    let report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "dynamic-index".into(),
        )
        .unwrap();
    assert_eq!(report.overview.attempt_count, 2);
    assert_eq!(report.quality.retry_reentry_rate.numerator, 0);
    assert_eq!(report.efficiency.observed_terminal_run_active_seconds, 24);
    assert_eq!(report.efficiency.active_duration_zero_filled_count, 0);
    assert_eq!(report.token_usage.total_tokens, 40);
    assert_eq!(
        report
            .efficiency
            .node_aggregates
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["worker-b", "worker-a"]
    );

    drop(connection);
    drop(index);
    let legacy = Connection::open(db.as_std_path()).unwrap();
    legacy
        .execute(
            "UPDATE analytics_index_state SET schemaVersion = 6 WHERE singleton = 1",
            [],
        )
        .unwrap();
    drop(legacy);

    let mut migrated = PersonalAnalyticsIndex::open(&db).unwrap();
    let migrated_state = migrated.state().unwrap();
    assert_eq!(migrated_state.schema_version, 9);
    assert_eq!(migrated_state.index_revision, 0);
    let migrated_stats = migrated.sync(&projects, |_, _| {}, || false).unwrap();
    assert!(migrated_stats.reparsed_files > 0);
    let migrated_report = migrated
        .report(
            &PersonalAnalyticsDateRange::default(),
            "dynamic-migrated".into(),
        )
        .unwrap();
    assert_eq!(migrated_report.overview.attempt_count, 2);
    assert_eq!(migrated_report.quality.retry_reentry_rate.numerator, 0);
}

#[test]
fn sqlite_usage_journals_update_attempts_without_inventing_nodes() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-1");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","title":"Usage task"}"#,
    );
    write(
        &task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow"}"#,
    );
    let attempt = task.join("runs/run-1/attempt-1");
    write(
        &task.join("runs/run-1/run.json"),
        r#"{"version":"1.0","status":"completed","outcome":"success","updated_at":"2026-08-18T02:00:00Z"}"#,
    );
    write(
        &attempt.join("node.json"),
        r#"{"version":"1.0","node_id":"plan","resolved_config":{"provider":"agent-a"}}"#,
    );
    write(
        &attempt.join("acp.snapshot.json"),
        r#"{"version":"1.0","timing":{"sessionElapsedSeconds":60}}"#,
    );
    write(
        &attempt.join("acp.prompt-usage.jsonl"),
        concat!(
            r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:00Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:01Z","usage":{"inputTokens":100,"outputTokens":50,"totalTokens":150}}"#,
            "\n",
            r#"{"kind":"promptStarted","turn_id":"turn-2","turn_seq":2,"timestamp":"2026-08-18T02:00:01Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-2","turn_seq":2,"timestamp":"2026-08-18T02:00:02Z","usage":{"inputTokens":60,"outputTokens":40,"totalTokens":100}}"#,
        ),
    );
    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();

    let connection = Connection::open(db.as_std_path()).unwrap();
    let attempt_row = connection
        .query_row(
            "SELECT nodeSourcePath IS NOT NULL, snapshotSourcePath IS NOT NULL,
                    usageSourcePath IS NOT NULL, totalTokens
             FROM analytics_attempts",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(attempt_row, (true, true, true, 250));
    let all = index
        .report(&PersonalAnalyticsDateRange::default(), "usage-all".into())
        .unwrap();
    assert_eq!(all.overview.attempt_count, 1);
    assert_eq!(all.quality.retry_reentry_rate.numerator, 0);
    assert_eq!(all.token_usage.observed_prompt_count, 2);
    assert_eq!(all.token_usage.total_tokens, 250);
    assert_eq!(all.efficiency.observed_terminal_run_active_seconds, 60);
    assert_eq!(all.efficiency.active_duration_zero_filled_count, 0);

    write(
        &attempt.join("acp.prompt-usage.jsonl"),
        concat!(
            r#"{"kind":"promptStarted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:02Z"}"#,
            "\n",
            r#"{"kind":"promptCompleted","turn_id":"turn-1","turn_seq":1,"timestamp":"2026-08-18T02:00:03Z","usage":{"inputTokens":120,"outputTokens":80,"totalTokens":200}}"#,
        ),
    );
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let updated_usage = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "usage-updated".into(),
        )
        .unwrap();
    assert_eq!(updated_usage.token_usage.total_tokens, 200);
    assert_eq!(updated_usage.token_usage.observed_prompt_count, 1);
    assert_eq!(updated_usage.overview.attempt_count, 1);

    write(
        &attempt.join("acp.snapshot.json"),
        r#"{"version":"1.0","timing":{"sessionElapsedSeconds":90}}"#,
    );
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let updated_snapshot = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "snapshot-updated".into(),
        )
        .unwrap();
    assert_eq!(
        updated_snapshot
            .efficiency
            .observed_terminal_run_active_seconds,
        90
    );
    assert_eq!(updated_snapshot.token_usage.total_tokens, 200);

    write(
        &attempt.join("node.json"),
        r#"{"version":"1.0","node_id":"plan-v2","resolved_config":{"provider":"agent-b"}}"#,
    );
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let updated_node = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "node-updated".into(),
        )
        .unwrap();
    assert_eq!(updated_node.overview.attempt_count, 1);
    assert_eq!(
        updated_node.efficiency.observed_terminal_run_active_seconds,
        90
    );
    assert_eq!(updated_node.token_usage.total_tokens, 200);

    fs::remove_file(attempt.join("acp.prompt-usage.jsonl")).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let usage_removed = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "usage-removed".into(),
        )
        .unwrap();
    assert_eq!(usage_removed.token_usage.total_tokens, 0);
    assert_eq!(usage_removed.overview.attempt_count, 1);
    assert_eq!(
        usage_removed
            .efficiency
            .observed_terminal_run_active_seconds,
        90
    );
}

#[test]
fn sqlite_sync_cancel_leaves_no_partial_index() {
    let root = tempdir().unwrap();
    for task_number in 0..8 {
        write(
            &root
                .path()
                .join(format!("project-a/tasks/task-{task_number}/task.json")),
            r#"{"version":"1.0","title":"Cancelled task"}"#,
        );
    }
    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let error = index
        .sync(&projects, |_, _| {}, || true)
        .expect_err("cancelled sync must fail before commit");
    assert!(error.to_string().contains("analytics.cancelled"));
    let connection = Connection::open(db.as_std_path()).unwrap();
    let source_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM analytics_sources", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(source_count, 0);
    let index_revision: i64 = connection
        .query_row(
            "SELECT indexRevision FROM analytics_index_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_revision, 0);
    drop(index);
    let mut reopened = PersonalAnalyticsIndex::open(&db).unwrap();
    let recovered = reopened.sync(&projects, |_, _| {}, || false).unwrap();
    assert_eq!(recovered.reparsed_files, 8);
    assert_eq!(recovered.index_revision, 1);
    let recovered_sources: i64 = Connection::open(db.as_std_path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM analytics_sources", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(recovered_sources, 8);
}
#[test]
#[ignore = "requires an explicit local MALING_PROJECTS_ROOT and a temporary SQLite database"]
fn real_history_sqlite_index_and_range_query_baseline() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
    let root = std::env::var("MALING_PROJECTS_ROOT")
        .expect("MALING_PROJECTS_ROOT must identify the local projects directory");
    let database = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(database.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let full = index
        .sync(Utf8Path::new(&root), |_, _| {}, || false)
        .unwrap();
    let increment = index
        .sync(Utf8Path::new(&root), |_, _| {}, || false)
        .unwrap();
    let range_started = Instant::now();
    let report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-01".into()),
                end: Some("2026-08-19".into()),
            },
            "performance-range".into(),
        )
        .unwrap();
    let range_ms = range_started.elapsed().as_millis();
    let all_report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "performance-all".into(),
        )
        .unwrap();
    println!(
        "sqlite_full_ms={} sqlite_full_index_revision={} sqlite_increment_ms={} sqlite_increment_reparsed={} sqlite_range_ms={} sqlite_range_runs={} attempts={} direct_started={} direct_completed={} direct_unknown={}",
        full.duration_ms,
        full.index_revision,
        increment.duration_ms,
        increment.reparsed_files,
        range_ms,
        report.overview.run_count,
        all_report.overview.attempt_count,
        all_report
            .reliability
            .direct_reply_completion_rate
            .denominator,
        all_report
            .reliability
            .direct_reply_completion_rate
            .numerator,
        all_report
            .reliability
            .direct_reply_completion_rate
            .unknown_count,
    );
    assert!(increment.reparsed_files < full.reparsed_files);
    if increment.reparsed_files == 0 {
        assert_eq!(increment.index_revision, full.index_revision);
    } else {
        assert!(increment.index_revision > full.index_revision);
    }
}

#[test]
#[ignore = "requires an explicit local MALING_PROJECTS_ROOT"]
fn real_history_scan_baseline() {
    let root = std::env::var("MALING_PROJECTS_ROOT")
        .expect("MALING_PROJECTS_ROOT must identify the local projects directory");
    let started = Instant::now();
    let output = build_personal_analytics_projection(
        Utf8Path::new(&root),
        "performance-baseline".to_string(),
        |_, _| {},
        || false,
    )
    .unwrap();
    println!(
        "elapsed_ms={} discovered_files={} parsed_files={} skipped_files={} discovered_bytes={} semantic_samples={} recent_tasks={} zero_filled_durations={} node_aggregates={}",
        started.elapsed().as_millis(),
        output.projection.source_coverage.discovered_files,
        output.projection.source_coverage.parsed_files,
        output.projection.source_coverage.skipped_files,
        output.projection.source_coverage.discovered_bytes,
        output.projection.source_coverage.semantic_sampled_items,
        output.projection.recent_tasks.len(),
        output
            .projection
            .efficiency
            .active_duration_zero_filled_count,
        output.projection.efficiency.node_aggregates.len(),
    );
}

#[test]
fn date_ranges_use_inclusive_local_natural_days() {
    let root = tempdir().unwrap();
    let task = root.path().join("project-a/tasks/task-local");
    write(
        &task.join("task.json"),
        r#"{"version":"1.0","id":"task-local"}"#,
    );
    write(
        &task.join("conversation.json"),
        r#"{"version":"1.0","runMode":"workflow","createdAt":"2026-08-18T00:00:00Z"}"#,
    );
    let midnight = Local
        .with_ymd_and_hms(2026, 8, 18, 0, 0, 0)
        .single()
        .unwrap();
    let timestamps = [
        (midnight - Duration::seconds(1)).to_rfc3339(),
        midnight.to_rfc3339(),
        (midnight + Duration::days(1) - Duration::seconds(1)).to_rfc3339(),
        (midnight + Duration::days(1)).to_rfc3339(),
    ];
    for (index, updated_at) in timestamps.iter().enumerate() {
        let run = task.join(format!("runs/run-{index}"));
        write(
            &run.join("run.json"),
            &format!(
                r#"{{"version":"1.0","status":"completed","outcome":"success","updated_at":"{updated_at}"}}"#
            ),
        );
        write(
            &run.join("attempt-1/node.json"),
            r#"{"version":"1.0","node_id":"plan"}"#,
        );
    }
    let projects = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    index.sync(&projects, |_, _| {}, || false).unwrap();
    let report = index
        .report(
            &PersonalAnalyticsDateRange {
                start: Some("2026-08-18".into()),
                end: Some("2026-08-18".into()),
            },
            "local-day".into(),
        )
        .unwrap();
    assert_eq!(
        report
            .reliability
            .workflow_run_terminal_success_rate
            .numerator,
        2
    );
    assert_eq!(
        report
            .reliability
            .workflow_run_terminal_success_rate
            .denominator,
        2
    );
}

#[test]
fn insight_cache_is_identity_scoped_and_bounded() {
    let root = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let narrative = PersonalAnalyticsNarrative {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        insights: Vec::new(),
    };
    for revision in 1..=68 {
        let identity = insight_identity(revision);
        index
            .store_completed_insight(
                &identity,
                &narrative,
                &format!("2026-08-18T00:{revision:02}:01Z"),
            )
            .unwrap();
    }
    assert!(
        index
            .completed_insight(&insight_identity(68))
            .unwrap()
            .is_some()
    );
    assert!(
        index
            .completed_insight(&insight_identity(4))
            .unwrap()
            .is_none()
    );
    assert!(
        index
            .completed_insight(&insight_identity(69))
            .unwrap()
            .is_none()
    );

    let completed_narrative = cached_narrative();
    let identity = insight_identity(68);
    index
        .store_completed_insight(&identity, &completed_narrative, "2026-08-18T01:00:00Z")
        .unwrap();
    let mut cache_hit_identity = insight_identity(68);
    cache_hit_identity.operation_id = "operation-cache-hit".into();
    assert_eq!(
        index.completed_insight(&cache_hit_identity).unwrap(),
        Some(completed_narrative.clone())
    );
    let deterministic_report = index
        .report(
            &PersonalAnalyticsDateRange::default(),
            "insights-report".into(),
        )
        .unwrap();
    assert!(deterministic_report.insights.is_empty());

    let connection = Connection::open(db.as_std_path()).unwrap();
    let retained: i64 = connection
        .query_row("SELECT COUNT(*) FROM analytics_insight_cache", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(retained, 64);
    let insight_view_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM analytics_insights", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(insight_view_count, 1);
}

#[test]
fn insight_cache_lookup_never_creates_a_parallel_lifecycle_row() {
    let root = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let mut identity = insight_identity(1);
    identity.operation_id = "operation-1".into();
    assert!(index.completed_insight(&identity).unwrap().is_none());
    index
        .store_completed_insight(&identity, &cached_narrative(), "2026-08-20T00:00:01Z")
        .unwrap();

    identity.operation_id = "operation-cache-hit".into();
    assert_eq!(
        index.completed_insight(&identity).unwrap(),
        Some(cached_narrative())
    );
    let connection = Connection::open(db.as_std_path()).unwrap();
    let run_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM analytics_insight_cache
             WHERE rangeStart IS ?1 AND rangeEnd IS ?2 AND schemaVersion = ?3
               AND indexRevision = ?4 AND agentType = ?5 AND modelId IS ?6
               AND thoughtLevelOptionId IS ?7 AND thoughtLevelValue IS ?8",
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
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(run_count, 1);
}

#[test]
fn insight_cache_identity_distinguishes_selected_models() {
    let root = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let mut model_a = insight_identity(1);
    model_a.model_id = Some("model-a".to_string());
    index
        .store_completed_insight(&model_a, &cached_narrative(), "2026-08-20T00:00:01Z")
        .unwrap();

    let mut same_model = model_a.clone();
    same_model.operation_id = "operation-same-model".into();
    assert!(index.completed_insight(&same_model).unwrap().is_some());

    let mut model_b = model_a.clone();
    model_b.operation_id = "operation-model-b".into();
    model_b.model_id = Some("model-b".to_string());
    assert!(index.completed_insight(&model_b).unwrap().is_none());

    let mut agent_default = model_a;
    agent_default.operation_id = "operation-agent-default".into();
    agent_default.model_id = None;
    assert!(index.completed_insight(&agent_default).unwrap().is_none());
}

#[test]
fn insight_cache_identity_distinguishes_selected_thought_levels() {
    let root = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(root.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let mut high = insight_identity(1);
    high.thought_level_option_id = Some("reasoning_effort".to_string());
    high.thought_level_value = Some("high".to_string());
    index
        .store_completed_insight(&high, &cached_narrative(), "2026-08-20T00:00:01Z")
        .unwrap();

    let mut same_level = high.clone();
    same_level.operation_id = "operation-same-level".into();
    assert!(index.completed_insight(&same_level).unwrap().is_some());

    let mut low = high.clone();
    low.operation_id = "operation-low".into();
    low.thought_level_value = Some("low".to_string());
    assert!(index.completed_insight(&low).unwrap().is_none());

    let mut unspecified = high;
    unspecified.operation_id = "operation-unspecified-level".into();
    unspecified.thought_level_option_id = None;
    unspecified.thought_level_value = None;
    assert!(index.completed_insight(&unspecified).unwrap().is_none());
}

fn cached_narrative() -> PersonalAnalyticsNarrative {
    PersonalAnalyticsNarrative {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        insights: vec![PersonalAnalyticsInsight {
            section: AnalyticsInsightSection::Quality,
            title: "收敛重试根因".into(),
            summary: "重试集中在少数节点。".into(),
            recommendation: "先补充失败隔离测试。".into(),
            confidence: AnalyticsInsightConfidence::High,
            sample_count: 8,
            evidence_locators: vec!["project-a/tasks/task-1/runs/run-1/node.json".into()],
        }],
    }
}
fn insight_identity(index_revision: u64) -> gold_band::personal_analytics::index::InsightIdentity {
    gold_band::personal_analytics::index::InsightIdentity {
        operation_id: format!("operation-{index_revision}"),
        range_start: None,
        range_end: None,
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        index_revision,
        agent_type: "agent-a".to_string(),
        model_id: None,
        thought_level_option_id: None,
        thought_level_value: None,
    }
}
