use std::fs;
use std::path::Path;
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{Duration, Local, TimeZone};
use gold_band::personal_analytics::{
    AnalyticsInsightConfidence, AnalyticsInsightSection, PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION,
    PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS, PersonalAnalyticsInsight,
    PersonalAnalyticsNarrative, build_personal_analytics_projection,
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
        &project.join("tasks/task-direct/turns/turn-1/turn.json"),
        r#"{"record":{"data":{"status":"completed"}}}"#,
    );
    write(
        &project.join("tasks/task-direct/turns/turn-2/turn.json"),
        r#"{"record":{"data":{"status":"cancelled"}}}"#,
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
        &"x".repeat(PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS + 500),
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
fn sqlite_index_syncs_incrementally_and_queries_date_ranges() {
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
    write(
        &run.join("attempt-1/node.json"),
        r#"{"version":"1.0","node_id":"plan","resolved_config":{"provider":"agent-a"}}"#,
    );
    write(
        &run.join("attempt-1/acp.snapshot.json"),
        r#"{"version":"1.0","timing":{"sessionElapsedSeconds":60}}"#,
    );
    write(
        &task.join("acp.prompt-usage.jsonl"),
        r#"{"usage":{"inputTokens":100,"outputTokens":50,"totalTokens":150}}"#,
    );
    write(
        &task.join("acp.timeline.jsonl"),
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
    assert_eq!(all.index_revision, first.index_revision);
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
    write(&task.join("turn.json"), "{ not json");
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
                "project-a/tasks/task-1/turn.json".to_string(),
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
    let root = std::env::var("MALING_PROJECTS_ROOT")
        .expect("MALING_PROJECTS_ROOT must identify the local projects directory");
    let database = tempdir().unwrap();
    let db = Utf8PathBuf::from_path_buf(database.path().join("gold-band.db")).unwrap();
    let mut index = PersonalAnalyticsIndex::open(&db).unwrap();
    let full_started = Instant::now();
    let full = index
        .sync(Utf8Path::new(&root), |_, _| {}, || false)
        .unwrap();
    let increment_started = Instant::now();
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
    println!(
        "sqlite_full_ms={} sqlite_full_index_revision={} sqlite_increment_ms={} sqlite_increment_reparsed={} sqlite_range_ms={} sqlite_range_runs={}",
        full_started.elapsed().as_millis(),
        full.index_revision,
        increment_started.elapsed().as_millis(),
        increment.reparsed_files,
        range_started.elapsed().as_millis(),
        report.overview.run_count,
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
    let index = PersonalAnalyticsIndex::open(&db).unwrap();
    let narrative = PersonalAnalyticsNarrative {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        insights: Vec::new(),
    };
    for revision in 1..=68 {
        let identity = insight_identity(revision);
        index
            .begin_insight(&identity, &format!("2026-08-18T00:{revision:02}:00Z"))
            .unwrap();
        index
            .finish_insight(
                &identity.operation_id,
                &narrative,
                "completed",
                None,
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
    index
        .finish_insight(
            "operation-68",
            &completed_narrative,
            "completed",
            None,
            "2026-08-18T01:00:00Z",
        )
        .unwrap();
    let mut cache_hit_identity = insight_identity(68);
    cache_hit_identity.operation_id = "operation-cache-hit".into();
    index
        .begin_insight(&cache_hit_identity, "2026-08-18T01:00:01Z")
        .unwrap();
    assert_eq!(
        index.completed_insight(&cache_hit_identity).unwrap(),
        Some(completed_narrative.clone())
    );

    let failed_identity = insight_identity(70);
    index
        .begin_insight(&failed_identity, "2026-08-18T01:00:02Z")
        .unwrap();
    index
        .finish_insight(
            &failed_identity.operation_id,
            &PersonalAnalyticsNarrative {
                schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
                insights: Vec::new(),
            },
            "failed",
            Some("analytics.report-invalid"),
            "2026-08-18T01:00:03Z",
        )
        .unwrap();
    assert!(index.completed_insight(&failed_identity).unwrap().is_none());
    let mut retry_identity = failed_identity;
    retry_identity.operation_id = "operation-retry".into();
    index
        .begin_insight(&retry_identity, "2026-08-18T01:00:04Z")
        .unwrap();
    index
        .finish_insight(
            &retry_identity.operation_id,
            &completed_narrative,
            "completed",
            None,
            "2026-08-18T01:00:05Z",
        )
        .unwrap();
    assert_eq!(
        index.completed_insight(&retry_identity).unwrap(),
        Some(cached_narrative())
    );

    let cancelled_identity = insight_identity(71);
    index
        .begin_insight(&cancelled_identity, "2026-08-18T01:00:06Z")
        .unwrap();
    index
        .finish_insight(
            &cancelled_identity.operation_id,
            &PersonalAnalyticsNarrative {
                schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
                insights: Vec::new(),
            },
            "cancelled",
            None,
            "2026-08-18T01:00:07Z",
        )
        .unwrap();
    assert!(
        index
            .completed_insight(&cancelled_identity)
            .unwrap()
            .is_none()
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
        .query_row("SELECT COUNT(*) FROM analytics_insight_runs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(retained, 64);
    let insight_view_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM analytics_insights", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(insight_view_count, 2);
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
    }
}
