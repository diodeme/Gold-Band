use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::console::controller::{
    activate_current, cycle_focus, escape, move_right, refresh_tick, show_help_overlay,
    start_command_input, toggle_log_source,
};
use gold_band::console::state::{
    ConsoleState, DetailLevel, DetailSelection, FocusPane, LayoutMode, WelcomeAction,
    WorkspaceSelection,
};
use gold_band::console::view_models::build_view_model;
use tempfile::tempdir;

fn seed_branching_repo(repo_root: &Utf8PathBuf) -> App {
    let app = App::new(repo_root.clone());
    std::fs::create_dir_all(
        app.paths
            .task_dir("task-001")
            .join("authoring")
            .as_std_path(),
    )
    .unwrap();
    let dev_profile = app
        .profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id;
    std::fs::create_dir_all(
        app.paths
            .artifacts_dir("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
    )
    .unwrap();
    std::fs::create_dir_all(
        app.paths
            .attachments_dir("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
    )
    .unwrap();
    std::fs::write(
        app.paths.task_file("task-001").as_std_path(),
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"branching workflow"}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths.workflow_file("task-001").as_std_path(),
        format!(
            r#"{{"version":"0.1","id":"full-flow","entry":"dev","control":{{"max_attempts":1}},"nodes":[{{"type":"worker","id":"dev","provider":"claude-acp","profile":"{}","primary_artifact":"implementation-result"}},{{"type":"worker","id":"review","provider":"claude-acp","profile":"{}","primary_artifact":"review-result","output":{{"kind":"json","artifact":"review-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}},{{"type":"worker","id":"accept","provider":"claude-acp","profile":"{}","primary_artifact":"accept-result","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}],"edges":[{{"from":"dev","to":"review","on":"success"}},{{"from":"dev","to":"accept","on":"failure"}},{{"from":"review","to":"accept","on":"invalid"}}]}}"#,
            dev_profile, dev_profile, dev_profile
        ),
    )
    .unwrap();
    std::fs::write(
        app.paths.run_file("task-001", "run-001").as_std_path(),
        r#"{"version":"0.1","id":"run-001","task_id":"task-001","status":"paused","outcome":null,"started_at":"2026-03-30T10:00:00Z","updated_at":"2026-03-30T10:01:00Z","workflow_snapshot":"workflow.snapshot.json","current_round":"round-001","current_node":"dev","current_attempt":"attempt-001","acceptance_loops_used":0,"pause_reason":"process-interrupted"}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths.run_progress_file("task-001", "run-001").as_std_path(),
        r#"{"status":"running","current_round":"round-001","current_node":"dev","current_attempt":"attempt-001"}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths
            .run_events_file("task-001", "run-001")
            .as_std_path(),
        "node-started\nprovider-streaming",
    )
    .unwrap();
    std::fs::write(
        app.paths.round_file("task-001", "run-001", "round-001").as_std_path(),
        r#"{"version":"0.1","id":"round-001","run_id":"run-001","index":1,"status":"paused","outcome":null,"trigger":"initial","started_at":"2026-03-30T10:00:00Z"}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths.node_file("task-001", "run-001", "round-001", "dev", "attempt-001").as_std_path(),
        r#"{"version":"0.1","node_id":"dev","node_type":"worker","run_id":"run-001","round_id":"round-001","attempt_id":"attempt-001","status":"paused","outcome":null,"started_at":"2026-03-30T10:00:00Z","finished_at":null,"resolved_config":{}}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths
            .progress_events_file("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
        "progress-line",
    )
    .unwrap();
    std::fs::write(
        app.paths
            .raw_stream_file("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
        "raw-line",
    )
    .unwrap();
    std::fs::write(
        app.paths
            .artifact_file(
                "task-001",
                "run-001",
                "round-001",
                "dev",
                "attempt-001",
                "implementation-result",
            )
            .as_std_path(),
        "result-body",
    )
    .unwrap();
    std::fs::write(
        app.paths
            .attachments_dir("task-001", "run-001", "round-001", "dev", "attempt-001")
            .join("stdout.txt")
            .as_std_path(),
        "stdout-body",
    )
    .unwrap();
    app
}

fn open_workspace(app: &App) -> ConsoleState {
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(app, &mut state).unwrap();
    activate_current(app, &mut state).unwrap();
    state
}

#[test]
fn workspace_renders_dag_with_edge_markers() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let state = open_workspace(&app);
    let vm = build_view_model(&app, &state).unwrap();
    let dag = vm.body_lines.join("\n");
    assert!(dag.contains("dev"));
    assert!(dag.contains("review"));
    assert!(dag.contains("accept"));
}

#[test]
fn entering_node_moves_focus_to_detail_and_shows_attempts() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let state = open_workspace(&app);
    assert_eq!(state.focus, FocusPane::Detail);
    let workspace = state.workspace.as_ref().unwrap();
    assert!(matches!(
        workspace.detail_level,
        DetailLevel::AttemptItems { .. }
    ));
    assert!(workspace.detail_items.iter().any(|item| matches!(
        item,
        DetailSelection::Artifact { .. } | DetailSelection::Attachment { .. }
    )));
}

#[test]
fn entering_attempt_then_artifact_supports_escape_backtracking() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    {
        let workspace = state.workspace.as_ref().unwrap();
        assert!(matches!(
            workspace.detail_level,
            DetailLevel::AttemptItems { .. }
        ));
    }
    activate_current(&app, &mut state).unwrap();
    {
        let workspace = state.workspace.as_ref().unwrap();
        assert_eq!(workspace.detail_level, DetailLevel::Content);
    }
    escape(&app, &mut state).unwrap();
    {
        let workspace = state.workspace.as_ref().unwrap();
        assert!(matches!(
            workspace.detail_level,
            DetailLevel::AttemptItems { .. }
        ));
    }
    escape(&app, &mut state).unwrap();
    {
        let workspace = state.workspace.as_ref().unwrap();
        assert_eq!(workspace.detail_level, DetailLevel::NodeHome);
    }
}

#[test]
fn workspace_header_and_detail_surface_run_progress() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let state = open_workspace(&app);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.header.contains("status=running"));
    let detail = vm.detail_body;
    assert!(detail.contains("Attempt: attempt-001"));
    assert!(detail.contains("Follow live: true"));
    assert!(detail.contains("Provider input snapshot (progress.events.jsonl)"));
    assert!(detail.contains("progress-line"));
}

#[test]
fn toggle_log_source_switches_attempt_detail_view() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    state.focus = FocusPane::Detail;

    toggle_log_source(&mut state);

    let vm = build_view_model(&app, &state).unwrap();
    assert!(
        vm.detail_body
            .contains("Provider output (raw.stream.jsonl)")
    );
    assert!(vm.detail_body.contains("raw-line"));
}

#[test]
fn attempt_detail_tolerates_missing_attempt_state_during_startup() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    std::fs::remove_file(
        app.paths
            .node_file("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
    )
    .unwrap();
    let state = open_workspace(&app);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.detail_body.contains("pending-persist"));
    assert!(vm.detail_body.contains("Waiting for runtime persistence"));
}

#[test]
fn refresh_tick_preserves_live_attempt_detail_mode() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    state.focus = FocusPane::Detail;
    {
        let workspace = state.workspace.as_mut().unwrap();
        workspace.detail_level = DetailLevel::AttemptItems {
            attempt_id: "attempt-001".to_string(),
            follow_live: true,
        };
    }

    refresh_tick(&app, &mut state).unwrap();

    let workspace = state.workspace.as_ref().unwrap();
    assert!(matches!(
        workspace.detail_level,
        DetailLevel::AttemptItems {
            follow_live: true,
            ..
        }
    ));
}

#[test]
fn refresh_tick_updates_workspace_progress() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    std::fs::write(
        app.paths.run_progress_file("task-001", "run-001").as_std_path(),
        r#"{"status":"paused","current_round":"round-001","current_node":"dev","current_attempt":"attempt-001"}"#,
    )
    .unwrap();

    refresh_tick(&app, &mut state).unwrap();

    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.header.contains("status=paused"));
}

#[test]
fn overlay_focus_and_compact_layout_work_in_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    show_help_overlay(&app, &mut state).unwrap();
    assert_eq!(state.focus, FocusPane::Overlay);
    assert!(state.overlay.is_some());
    escape(&app, &mut state).unwrap();
    assert_eq!(state.focus, FocusPane::Detail);
    start_command_input(&mut state);
    assert_eq!(state.focus, FocusPane::Input);
    state.layout_mode = LayoutMode::Compact;
    state.focus = FocusPane::Detail;
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.compact_detail_only);
    cycle_focus(&mut state);
    assert_eq!(state.focus, FocusPane::Input);
}

#[test]
fn dag_navigation_moves_between_columns() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = seed_branching_repo(&repo_root);
    let mut state = open_workspace(&app);
    move_right(&mut state);
    let workspace = state.workspace.as_ref().unwrap();
    assert!(matches!(
        workspace.selection,
        WorkspaceSelection::Node { .. }
    ));
}
