use camino::Utf8PathBuf;
use gold_band::app::App;
use gold_band::config::ConsoleThemeName;
use gold_band::console::controller::{
    activate_current, cycle_focus, escape, move_down, show_help_overlay, start_command_input,
    start_selected_task, submit_input,
};
use gold_band::console::state::{ConsoleState, FocusPane, LayoutMode, Screen, WelcomeAction};
use gold_band::console::view_models::build_view_model;
use gold_band::domain::SessionMode;
use gold_band::inspect::render_console_banner;
use gold_band::provider::{
    DoctorResult, PrimaryArtifactPayload, ProviderAdapter, ProviderCapabilities, ProviderInfo,
    ProviderResultPayload, ProviderRunResult, ProviderRunStatus, SessionRef, WorkerInvocation,
};
use tempfile::tempdir;

#[derive(Clone, Default)]
struct StartTaskProvider;

impl ProviderAdapter for StartTaskProvider {
    fn describe_provider(&self) -> ProviderInfo {
        ProviderInfo {
            provider_id: "fake".to_string(),
            display_name: "Fake".to_string(),
            capabilities: ProviderCapabilities {
                supports_open_session: true,
                supports_continue_session: true,
                supports_raw_stream: false,
            },
            is_default: false,
        }
    }

    fn doctor(&self) -> DoctorResult {
        DoctorResult {
            available: true,
            reason: None,
            capabilities: None,
        }
    }

    fn run_worker(&self, req: WorkerInvocation) -> anyhow::Result<ProviderRunResult> {
        let payload = match req.primary_artifact.as_deref() {
            Some("implementation-result") => PrimaryArtifactPayload {
                name: "implementation-result".to_string(),
                content: r#"{"version":"0.1","commands":[{"id":"ok","run":"echo ok","purpose":"run checks"}]}"#.to_string(),
            },
            Some("accept-result") => PrimaryArtifactPayload {
                name: "accept-result".to_string(),
                content: r#"{"result":true,"reason":"accepted"}"#.to_string(),
            },
            _ => unreachable!(),
        };

        Ok(ProviderRunResult {
            status: ProviderRunStatus::Success,
            exit_code: Some(0),
            result_payload: Some(ProviderResultPayload {
                primary_artifact: Some(payload),
            }),
            worker_ref_seed: Some(SessionRef {
                provider: "claude-acp".to_string(),
                mode: SessionMode::New,
                supports_open_session: true,
                supports_continue_session: true,
                continue_ref: Some(serde_json::json!({"sessionId": "session-1"})),
                open_command: Some("claude -c session-1".to_string()),
            }),
            stream_path: None,
        })
    }

    fn open_session(&self, _worker_ref: &gold_band::domain::SessionRef) -> anyhow::Result<()> {
        Ok(())
    }

    fn build_continue_command(
        &self,
        _worker_ref: &gold_band::domain::SessionRef,
    ) -> anyhow::Result<Option<String>> {
        Ok(Some("claude -c session-1".to_string()))
    }
}

fn write_developer_profile(repo_root: &Utf8PathBuf) {
    std::fs::create_dir_all(repo_root.join(".gold-band/presets/profiles").as_std_path()).unwrap();
    std::fs::write(
        repo_root
            .join(".gold-band/presets/profiles/developer.md")
            .as_std_path(),
        "# developer",
    )
    .unwrap();
}

fn seed_task(app: &App, task_id: &str, task_json: &str, workflow_json: &str) {
    std::fs::create_dir_all(app.paths.task_dir(task_id).join("authoring").as_std_path()).unwrap();
    std::fs::write(app.paths.task_file(task_id).as_std_path(), task_json).unwrap();
    std::fs::write(
        app.paths.workflow_file(task_id).as_std_path(),
        workflow_json,
    )
    .unwrap();
}

fn seed_basic_task(app: &App, task_id: &str, description: &str) {
    std::fs::create_dir_all(app.paths.task_dir(task_id).as_std_path()).unwrap();
    std::fs::write(
        app.paths.task_file(task_id).as_std_path(),
        format!(r#"{{"version":"0.1","id":"{task_id}","description":"{description}"}}"#),
    )
    .unwrap();
}

fn developer_profile_id(app: &App) -> String {
    app.profiles()
        .unwrap()
        .profiles
        .into_iter()
        .find(|profile| profile.name == "开发")
        .unwrap()
        .id
}

fn worker_workflow(app: &App) -> String {
    format!(
        r#"{{"version":"0.1","id":"full-flow","entry":"dev","control":{{"max_attempts":1,"max_rounds":1}},"nodes":[{{"type":"worker","id":"dev","provider":"claude-acp","profile":"{}"}}],"edges":[]}}"#,
        developer_profile_id(app)
    )
}

#[test]
fn generated_banner_contains_multiple_lines() {
    let banner = render_console_banner();
    assert!(banner.lines().count() >= 3);
    assert!(!banner.trim().is_empty());
}

#[test]
fn welcome_screen_is_default_and_renders_primary_actions() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let state = ConsoleState::default();
    let vm = build_view_model(&app, &state).unwrap();
    assert_eq!(state.screen, Screen::Welcome);
    assert_eq!(state.focus, FocusPane::Welcome);
    assert!(vm.body_lines.iter().any(|line| line.contains("新增 task")));
    assert!(
        vm.body_lines
            .iter()
            .any(|line| line.contains("选择现有 task"))
    );
    assert!(!vm.show_detail);
    assert!(!vm.show_input);
}

#[test]
fn welcome_does_not_cycle_to_input() {
    let mut state = ConsoleState::default();
    cycle_focus(&mut state);
    assert_eq!(state.focus, FocusPane::Welcome);
}

#[test]
fn welcome_select_existing_task_enters_task_picker() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    assert_eq!(state.screen, Screen::TaskPicker);
    assert_eq!(state.task_list.len(), 1);
}

#[test]
fn slash_task_command_opens_task_picker() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    seed_basic_task(&app, "task-001", "demo task");
    let mut state = ConsoleState::default();
    state.screen = Screen::TaskPicker;
    start_command_input(&mut state);
    state.input = "/task".to_string();
    submit_input(&app, &mut state).unwrap();
    assert_eq!(state.screen, Screen::TaskPicker);
}

#[test]
fn task_picker_selection_with_active_run_enters_attempt_detail() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root.clone());
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    write_developer_profile(&repo_root);
    std::fs::create_dir_all(
        app.paths
            .attempt_dir("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
    )
    .unwrap();
    std::fs::write(
        app.paths.run_file("task-001", "run-001").as_std_path(),
        r#"{"version":"0.1","id":"run-001","task_id":"task-001","status":"running","outcome":null,"started_at":"2026-03-30T10:00:00Z","updated_at":"2026-03-30T10:01:00Z","workflow_snapshot":"workflow.snapshot.json","current_round":"round-001","current_node":"dev","current_attempt":"attempt-001","acceptance_loops_used":0,"pause_reason":null}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths.round_file("task-001", "run-001", "round-001").as_std_path(),
        r#"{"version":"0.1","id":"round-001","run_id":"run-001","index":1,"status":"running","outcome":null,"trigger":"initial","started_at":"2026-03-30T10:00:00Z"}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths
            .node_file("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
        r#"{"version":"0.1","node_id":"dev","node_type":"worker","run_id":"run-001","round_id":"round-001","attempt_id":"attempt-001","status":"running","outcome":null,"started_at":"2026-03-30T10:00:00Z","finished_at":null,"resolved_config":{"primaryArtifact":"implementation-result"}}"#,
    )
    .unwrap();
    std::fs::write(
        app.paths
            .progress_events_file("task-001", "run-001", "round-001", "dev", "attempt-001")
            .as_std_path(),
        "provider-input-line",
    )
    .unwrap();

    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    activate_current(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::Workspace);
    assert_eq!(state.focus, FocusPane::Detail);
    let workspace = state.workspace.as_ref().unwrap();
    assert!(matches!(
        workspace.detail_level,
        gold_band::console::state::DetailLevel::AttemptItems {
            follow_live: true,
            ..
        }
    ));
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.detail_body.contains("Attempt: attempt-001"));
}

#[test]
fn task_picker_selection_enters_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root.clone());
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    write_developer_profile(&repo_root);
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    activate_current(&app, &mut state).unwrap();
    assert_eq!(state.screen, Screen::Workspace);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.header.contains("task-001"));
    assert!(vm.show_detail);
}

#[test]
fn esc_from_workspace_returns_to_task_picker() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root.clone());
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    write_developer_profile(&repo_root);
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    activate_current(&app, &mut state).unwrap();
    assert_eq!(state.screen, Screen::Workspace);
    escape(&app, &mut state).unwrap();
    escape(&app, &mut state).unwrap();
    assert_eq!(state.screen, Screen::TaskPicker);
}

#[test]
fn question_mark_help_and_slash_command_entry_work_in_task_picker() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let mut state = ConsoleState::default();
    state.screen = Screen::TaskPicker;
    show_help_overlay(&app, &mut state).unwrap();
    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.show_overlay);
    assert!(vm.overlay_body.contains("Keyboard:"));
    assert!(
        vm.overlay_body
            .contains("start selected task (Task Picker)")
    );
    assert!(
        vm.overlay_body
            .contains("toggle log source (Attempt detail)")
    );
    assert!(
        vm.overlay_body
            .contains("Arrow keys       move selection; in attempt detail they scroll history")
    );
    assert!(
        vm.overlay_body
            .contains("/theme [gold-band|nord|dracula|cyber|onyx|mist|high-contrast]")
    );
    escape(&app, &mut state).unwrap();
    start_command_input(&mut state);
    let vm = build_view_model(&app, &state).unwrap();
    assert_eq!(state.focus, FocusPane::Input);
    assert_eq!(state.input, "/");
    assert!(vm.input_hint.contains("/task"));
    assert!(vm.input_hint.contains("/theme"));
}

#[test]
fn too_small_layout_hides_command_bar() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    let mut state = ConsoleState::default();
    state.viewport.width = 70;
    state.viewport.height = 20;
    state.layout_mode = LayoutMode::TooSmall;
    let vm = build_view_model(&app, &state).unwrap();
    assert!(!vm.show_input);
    assert!(
        vm.body_lines
            .iter()
            .any(|line| line.contains("Terminal too small"))
    );
}

#[test]
fn log_command_renders_output_in_task_picker_overlay() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    std::fs::create_dir_all(app.paths.logs_dir().as_std_path()).unwrap();
    std::fs::write(app.paths.runtime_log_file().as_std_path(), "line-1\nline-2").unwrap();
    let mut state = ConsoleState::default();
    state.screen = Screen::TaskPicker;
    start_command_input(&mut state);
    state.input = "/log".to_string();
    submit_input(&app, &mut state).unwrap();
    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.show_overlay);
    assert!(vm.overlay_body.contains("line-1"));
    assert!(!vm.show_input);
}

#[test]
fn start_selected_task_enters_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::with_provider(repo_root.clone(), Box::new(StartTaskProvider));
    let dev_profile = developer_profile_id(&app);
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"demo task"}"#,
        &format!(
            r#"{{
          "version": "0.1",
          "id": "full-flow",
          "entry": "dev",
          "control": {{ "max_attempts": 1 }},
          "nodes": [
            {{"id":"dev","type":"worker","provider":"claude-acp","profile":"{}","goal":"Create an implementation result","primary_artifact":"implementation-result"}},
            {{"id":"accept","type":"worker","provider":"claude-acp","profile":"{}","primary_artifact":"accept-result","output":{{"kind":"json","artifact":"accept-result","schema":{{"result":"boolean","reason":"String"}}}},"success_condition":{{"expression":"$.result == true"}}}}
          ],
          "edges": [
            {{"from":"dev","to":"accept","on":"success"}},
            {{"from":"accept","to":"$end","on":"success"}}
          ]
        }}"#,
            dev_profile, dev_profile
        ),
    );
    write_developer_profile(&repo_root);
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();

    start_selected_task(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::Workspace);
    assert_eq!(state.focus, FocusPane::Detail);
    assert!(state.message.is_none());
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.header.contains("task-001"));
    assert!(
        vm.detail_body.contains("Provider output")
            || vm.detail_body.contains("Attempt:")
            || vm.detail_body.contains("Attempts")
            || vm.detail_body.contains("Node: dev")
    );
}

#[test]
fn start_selected_task_marks_background_start() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root.clone());
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    write_developer_profile(&repo_root);
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();

    start_selected_task(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::Workspace);
    assert!(state.background_task.is_some());
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.header.contains("background: start pending"));
}

#[test]
fn enter_submits_task_picker_command_instead_of_opening_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","title":"Task One","description":"demo task"}"#,
        &worker_workflow(&app),
    );
    std::fs::create_dir_all(app.paths.logs_dir().as_std_path()).unwrap();
    std::fs::write(app.paths.runtime_log_file().as_std_path(), "line-1\nline-2").unwrap();
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    start_command_input(&mut state);
    state.input = "/log".to_string();

    activate_current(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::TaskPicker);
    assert!(state.workspace.is_none());
    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.show_overlay);
    assert!(vm.overlay_body.contains("line-1"));
}

#[test]
fn invalid_task_shows_reason_and_cannot_enter_workspace() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    seed_task(
        &app,
        "task-001",
        r#"{"version":"0.1","id":"task-001","description":"demo task"}"#,
        r#"{"version":"0.1","id":"full-flow","entry":"dev","control":{"max_attempts":1,"max_rounds":1},"nodes":[{"type":"worker","id":"dev","provider":"claude-acp","profile":"missing-profile"}],"edges":[]}"#,
    );
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    let vm = build_view_model(&app, &state).unwrap();
    assert!(
        vm.body_lines
            .iter()
            .any(|line| line.contains("workflow invalid"))
    );
    assert!(vm.body_lines.iter().any(|line| line.contains("reason:")));
    let rich_lines = vm.body_rich_lines.as_ref().unwrap();
    assert!(
        rich_lines
            .iter()
            .flatten()
            .any(|span| span.text.contains("workflow invalid"))
    );
    assert!(
        rich_lines
            .iter()
            .flatten()
            .any(|span| span.text.contains("reason: "))
    );

    activate_current(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::TaskPicker);
    assert!(state.workspace.is_none());
    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.show_overlay);
    assert!(vm.overlay_body.contains("not enterable yet"));
}

#[test]
fn move_down_changes_selected_task() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let app = App::new(repo_root);
    for id in ["task-001", "task-002"] {
        seed_basic_task(&app, id, "demo");
    }
    let mut state = ConsoleState::default();
    state.welcome_action = WelcomeAction::SelectTask;
    activate_current(&app, &mut state).unwrap();
    move_down(&mut state);
    assert_eq!(state.task_index, 1);
}

#[test]
fn theme_command_persists_theme_and_preserves_screen() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let home_dir = repo_root.join("fake-home");
    let mut app = App::new(repo_root.clone());
    app.paths.user_gold_band_root = home_dir.join(".gold-band");
    let mut state = ConsoleState::default();
    state.screen = Screen::TaskPicker;
    state.console_theme = ConsoleThemeName::GoldBand;
    start_command_input(&mut state);
    state.input = "/theme cyber".to_string();

    submit_input(&app, &mut state).unwrap();

    assert_eq!(state.screen, Screen::TaskPicker);
    assert_eq!(state.console_theme, ConsoleThemeName::Cyber);
    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.overlay_body.contains("Console theme switched to cyber"));
    assert!(vm.overlay_body.contains("Persisted user theme: cyber"));

    let persisted = app.load_user_config().unwrap();
    assert_eq!(persisted.console_theme, Some(ConsoleThemeName::Cyber));
}

#[test]
fn config_overlay_reports_startup_persisted_and_effective_theme() {
    let temp = tempdir().unwrap();
    let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let home_dir = repo_root.join("fake-home");
    let mut app = App::new(repo_root.clone());
    app.paths.user_gold_band_root = home_dir.join(".gold-band");
    app.set_user_console_theme(ConsoleThemeName::Nord).unwrap();
    let mut state = ConsoleState::default();
    state.screen = Screen::TaskPicker;
    state.console_theme = ConsoleThemeName::Dracula;
    start_command_input(&mut state);
    state.input = "/config".to_string();

    submit_input(&app, &mut state).unwrap();

    assert_eq!(state.focus, FocusPane::Overlay);
    let vm = build_view_model(&app, &state).unwrap();
    assert!(vm.overlay_body.contains("startup_theme: gold-band"));
    assert!(vm.overlay_body.contains("persisted_user_theme: nord"));
    assert!(vm.overlay_body.contains("effective_theme: dracula"));
    assert!(vm.overlay_body.contains("source: console command"));
}
