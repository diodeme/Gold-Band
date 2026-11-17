use super::{defaults::*, discovery::*, environment::EnvironmentReport, initialization, model::*};
use crate::{
    app::App,
    config::{ManagedAgentConfig, ProviderDiagnosticSnapshot, RuntimeConfig},
    storage::{load_settings_file, read_json, save_settings_file, write_json},
};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};

const PATH_CONFIG: crate::storage::StoragePathConfig = crate::storage::StoragePathConfig {
    app_key: "onboarding-setup-test",
    config_dir_name: "onboarding-setup-test",
    home_env_var: "GOLD_BAND_ONBOARDING_TEST_HOME",
};

fn fixture() -> (
    tempfile::TempDir,
    App,
    SetupCandidate,
    DefaultSetupSelection,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_owned()).unwrap();
    let paths = crate::storage::GoldBandPaths::new_with_path_config(root.clone(), PATH_CONFIG);
    let settings = initialization::initialize(&paths).unwrap();
    initialization::skip_git(&paths).unwrap();
    let app = App::with_config_and_path_config(
        root.clone(),
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    let candidate = SetupCandidate {
        id: "claude-acp".parse().unwrap(),
        config: ManagedAgentConfig::from_catalog(
            crate::agent_catalog::builtin_agent("claude-acp").unwrap(),
        ),
        expected_fingerprint: None,
    };
    let settings = register_success(
        &app.paths,
        &candidate,
        &diagnostic(&candidate),
        &AtomicBool::new(false),
    )
    .unwrap();
    let app = App::with_config_and_path_config(
        root,
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    app.initialize_workflow_templates().unwrap();
    let selection = DefaultSetupSelection {
        profile: DefaultExecutionProfile {
            agent_id: candidate.id.clone(),
            model_selection: ModelSelection::Explicit {
                model_id: "test-model".into(),
            },
            config_options: BTreeMap::new(),
            model_bound_overrides: BTreeMap::new(),
        },
        permission_mode_id: Some("default".into()),
    };
    (temp, app, candidate, selection)
}

fn diagnostic(candidate: &SetupCandidate) -> ProviderDiagnosticSnapshot {
    ProviderDiagnosticSnapshot {
        config_fingerprint: Some(candidate.config.diagnostic_fingerprint().unwrap()),
        available: true,
        error: None,
        checked_at: "2026-10-21T00:00:00Z".into(),
        capabilities: Some(serde_json::json!({
            "configOptions": [{"id":"model","type":"select","category":"model","currentValue":"test-model","options":[{"value":"test-model","name":"Test model"}]}],
            "modes": {"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}
        })),
    }
}

fn request(app: &App, selection: DefaultSetupSelection) -> ApplyDefaultSetup {
    ApplyDefaultSetup {
        preview_fingerprint: preview(app, &selection).unwrap().fingerprint,
        selection,
        operation_id: uuid::Uuid::new_v4().to_string(),
        git_skipped: true,
    }
}
fn prepared(
    candidate: &SetupCandidate,
    cwd: &std::path::Path,
) -> anyhow::Result<Option<crate::config::AcpAdapterConfig>> {
    Ok(match prepared_adapter(candidate, cwd, true)? {
        PreparedAdapter::Ready(adapter) => Some(adapter),
        PreparedAdapter::Unavailable(_) => None,
    })
}

fn ready() -> EnvironmentReport {
    EnvironmentReport {
        checks: vec![],
        npx_launchable: true,
    }
}

#[test]
fn deferred_setup_preserves_git_skip_for_later_apply() {
    let (_temp, app, _, selection) = fixture();
    let mut request = request(&app, selection);
    // The request no longer carries the Git decision; it must be recovered from
    // the persisted deferred onboarding state.
    request.git_skipped = false;

    initialization::defer(&app.paths).unwrap();

    let settings = apply(&app, &request, &ready()).unwrap();
    assert!(matches!(
        settings.onboarding,
        Some(OnboardingState::Completed { .. })
    ));
}

#[test]
fn setup_preview_is_read_only_apply_updates_all_slots_and_is_idempotent() {
    let (_temp, app, _, selection) = fixture();
    let settings_before = std::fs::read(app.paths.user_settings_file()).unwrap();
    let workflows_before = std::fs::read(app.paths.workflow_templates_file()).unwrap();
    let request = request(&app, selection);
    assert_eq!(
        settings_before,
        std::fs::read(app.paths.user_settings_file()).unwrap()
    );
    assert_eq!(
        workflows_before,
        std::fs::read(app.paths.workflow_templates_file()).unwrap()
    );
    apply(&app, &request, &ready()).unwrap();
    let store = app.read_workflow_templates().unwrap();
    assert_eq!(
        store.templates.len(),
        app.built_in_workflow_template_ids().len()
    );
    assert!(
        store
            .templates
            .iter()
            .flat_map(|t| &t.model_bindings.bindings)
            .all(|binding| binding.model_id.as_deref() == Some("test-model"))
    );
    let applied = std::fs::read(app.paths.workflow_templates_file()).unwrap();
    apply(
        &app,
        &request,
        &EnvironmentReport {
            npx_launchable: false,
            checks: vec![],
        },
    )
    .unwrap();
    assert!(applied == std::fs::read(app.paths.workflow_templates_file()).unwrap());
    assert!(matches!(
        app.load_settings().unwrap().onboarding,
        Some(OnboardingState::Completed { .. })
    ));
}

#[test]
fn setup_stale_preview_does_not_write() {
    let (_temp, app, _, selection) = fixture();
    let mut request = request(&app, selection);
    request.preview_fingerprint = "stale".into();
    assert!(
        apply(&app, &request, &ready())
            .unwrap_err()
            .to_string()
            .contains("preview-stale")
    );
    assert!(matches!(
        app.load_settings().unwrap().onboarding,
        Some(OnboardingState::Pending { .. })
    ));
}

#[test]
fn setup_environment_problems_never_block_apply_with_a_healthy_agent() {
    use super::environment::{
        EnvironmentCheck, EnvironmentError, EnvironmentStatus, EnvironmentTool,
    };
    let check = |tool, status, code: &str| EnvironmentCheck {
        tool,
        status,
        version: None,
        executable_path: None,
        node_executable_path: None,
        node_version: None,
        error: Some(EnvironmentError {
            code: code.into(),
            params: serde_json::json!({ "tool": tool }),
        }),
    };
    let (_temp, app, _, selection) = fixture();
    let request = request(&app, selection);
    let report = EnvironmentReport::from_checks(vec![
        check(
            EnvironmentTool::Node,
            EnvironmentStatus::Missing,
            "setup.runtime-missing",
        ),
        check(
            EnvironmentTool::Npm,
            EnvironmentStatus::Timeout,
            "setup.probe-timeout",
        ),
        check(
            EnvironmentTool::Npx,
            EnvironmentStatus::Missing,
            "setup.runtime-missing",
        ),
    ]);
    assert!(!report.npx_launchable);

    apply(&app, &request, &report).unwrap();

    assert!(matches!(
        app.load_settings().unwrap().onboarding,
        Some(OnboardingState::Completed { .. })
    ));
}

#[test]
fn setup_registration_preserves_other_agents_and_rejects_late_results() {
    let (_temp, app, candidate, selection) = fixture();
    let request = request(&app, selection);
    let other = SetupCandidate {
        id: "custom-local".parse().unwrap(),
        config: candidate.config.clone(),
        expected_fingerprint: None,
    };
    register_success(
        &app.paths,
        &other,
        &diagnostic(&other),
        &AtomicBool::new(false),
    )
    .unwrap();
    // Unrelated registration must neither stale the preview nor be lost on apply.
    let settings = apply(&app, &request, &ready()).unwrap();
    assert_eq!(settings.agents.as_ref().unwrap().len(), 2);
    let mut retry = other.clone();
    retry.expected_fingerprint = Some(fingerprint(&other.config).unwrap());
    register_success(
        &app.paths,
        &retry,
        &diagnostic(&retry),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut settings = load_settings_file(&app.paths.user_settings_file()).unwrap();
    settings.agents.as_mut().unwrap().remove(&other.id);
    save_settings_file(&app.paths.user_settings_file(), &settings).unwrap();
    assert!(
        register_success(
            &app.paths,
            &retry,
            &diagnostic(&retry),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        register_success(
            &app.paths,
            &other,
            &diagnostic(&other),
            &AtomicBool::new(true)
        )
        .is_err()
    );
}

#[test]
fn setup_requires_matching_diagnostics_and_failed_persistence_never_returns_ready() {
    let (_temp, app, candidate, selection) = fixture();
    let mut changed = load_settings_file(&app.paths.user_settings_file()).unwrap();
    changed
        .agents
        .as_mut()
        .unwrap()
        .get_mut(&candidate.id)
        .unwrap()
        .adapter
        .env
        .insert("CHANGED".into(), "1".into());
    save_settings_file(&app.paths.user_settings_file(), &changed).unwrap();
    assert!(
        preview(&app, &selection)
            .unwrap_err()
            .to_string()
            .contains("agent-unavailable")
    );
    let other = SetupCandidate {
        id: "new-local".parse().unwrap(),
        config: candidate.config,
        expected_fingerprint: None,
    };
    std::fs::remove_file(app.paths.agent_diagnostics_file()).unwrap();
    std::fs::create_dir(app.paths.agent_diagnostics_file()).unwrap();
    assert!(
        register_success(
            &app.paths,
            &other,
            &diagnostic(&other),
            &AtomicBool::new(false)
        )
        .is_err()
    );
}

#[test]
fn setup_keeps_user_templates_and_last_used_selection_unchanged() {
    let (_temp, app, _, selection) = fixture();
    let mut store = app.read_workflow_templates().unwrap();
    let mut custom = store.templates[0].clone();
    custom.id = "user-template".into();
    custom.is_built_in = false;
    store.templates.push(custom.clone());
    store.last_used_template_id = Some(custom.id.clone());
    write_json(&app.paths.workflow_templates_file(), &store).unwrap();
    let request = request(&app, selection);
    apply(&app, &request, &ready()).unwrap();
    let applied = app.read_workflow_templates().unwrap();
    assert_eq!(
        serde_json::to_value(applied.templates.last().unwrap()).unwrap(),
        serde_json::to_value(&custom).unwrap()
    );
    assert_eq!(applied.last_used_template_id, Some(custom.id));
}

#[test]
fn setup_unknown_model_and_implicit_model_are_rejected() {
    let (_temp, app, _, mut selection) = fixture();
    selection.profile.model_selection = ModelSelection::ProviderDefault;
    assert!(
        preview(&app, &selection)
            .unwrap_err()
            .to_string()
            .contains("model-selection-required")
    );
    selection.profile.model_selection = ModelSelection::Explicit {
        model_id: "invented".into(),
    };
    assert!(
        preview(&app, &selection)
            .unwrap_err()
            .to_string()
            .contains("model-config-invalid")
    );
}

#[test]
fn setup_legacy_catalog_without_a_standard_selector_accepts_provider_default() {
    for unrelated_config in [false, true] {
        let (_temp, app, mut candidate, mut selection) = fixture();
        let mut legacy = diagnostic(&candidate);
        let mut capabilities = serde_json::json!({
            "models": {
                "currentModelId": "legacy-current",
                "availableModels": [
                    {"modelId": "legacy-current", "name": "Current model"},
                    {"modelId": "legacy-other", "name": "Other model"}
                ]
            },
            "modes": {"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}]}
        });
        if unrelated_config {
            capabilities["configOptions"] = serde_json::json!([
                {"id": "output", "type": "select", "currentValue": "text", "options": [{"value": "text", "name": "Text"}]}
            ]);
        }
        legacy.capabilities = Some(capabilities);
        candidate.expected_fingerprint = Some(fingerprint(&candidate.config).unwrap());
        register_success(&app.paths, &candidate, &legacy, &AtomicBool::new(false)).unwrap();

        selection.profile.model_selection = ModelSelection::Explicit {
            model_id: "invented".into(),
        };
        assert!(
            preview(&app, &selection)
                .unwrap_err()
                .to_string()
                .contains("model-config-invalid")
        );
        selection.profile.model_selection = ModelSelection::ProviderDefault;
        let request = request(&app, selection);
        let settings = apply(&app, &request, &ready()).unwrap();
        assert_eq!(
            settings
                .default_execution_profile
                .unwrap()
                .model_selection
                .model_id(),
            None
        );
        let store = app.read_workflow_templates().unwrap();
        assert!(
            store
                .templates
                .iter()
                .any(|template| !template.model_bindings.bindings.is_empty())
        );
        assert!(
            store
                .templates
                .iter()
                .flat_map(|template| &template.model_bindings.bindings)
                .all(|binding| { binding.model_id.is_none() && binding.config_options.is_empty() })
        );
    }
}

#[test]
fn setup_switches_agents_with_safe_permissions_and_keeps_existing_slot_choices_on_upgrade() {
    let (_temp, app, candidate, mut selection) = fixture();
    let target = SetupCandidate {
        id: "new-agent".parse().unwrap(),
        config: candidate.config,
        expected_fingerprint: None,
    };
    register_success(
        &app.paths,
        &target,
        &diagnostic(&target),
        &AtomicBool::new(false),
    )
    .unwrap();
    // Establish real old bindings before testing the provider switch.
    apply(&app, &request(&app, selection.clone()), &ready()).unwrap();
    let mut store = app.read_workflow_templates().unwrap();
    for template in &mut store.templates {
        for binding in &mut template.model_bindings.bindings {
            binding.auto_accept = true;
            binding.permission_mode_id = Some("old-mode".into());
        }
    }
    write_json(&app.paths.workflow_templates_file(), &store).unwrap();
    selection.profile.agent_id = target.id;
    let request = request(&app, selection.clone());
    let settings = apply(&app, &request, &ready()).unwrap();
    let store = app.read_workflow_templates().unwrap();
    assert!(
        store
            .templates
            .iter()
            .flat_map(|t| &t.model_bindings.bindings)
            .all(|binding| !binding.auto_accept
                && binding.permission_mode_id.as_deref() == Some("default"))
    );
    let reloaded = App::with_config_and_path_config(
        app.paths.repo_root.clone(),
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    let before = std::fs::read(app.paths.workflow_templates_file()).unwrap();
    reloaded.initialize_workflow_templates().unwrap();
    assert!(
        before == std::fs::read(app.paths.workflow_templates_file()).unwrap(),
        "initialization must not rewrite committed bindings"
    );
    let initial = selection.profile.initial_run_mode();
    assert_eq!(initial.direct_config.unwrap().agent_type, "new-agent");
    assert_eq!(
        initial.auto_config.unwrap().model_id.as_deref(),
        Some("test-model")
    );
}

#[test]
fn setup_operation_identity_rejects_different_requests_and_template_edits_stale_preview() {
    let (_temp, app, _, selection) = fixture();
    let mut first = request(&app, selection.clone());
    apply(&app, &first, &ready()).unwrap();
    first.git_skipped = false;
    assert!(
        apply(&app, &first, &ready())
            .unwrap_err()
            .to_string()
            .contains("configuration-conflict")
    );
    let pending = request(&app, selection);
    let mut store = app.read_workflow_templates().unwrap();
    store.templates[0].model_bindings.bindings[0].auto_accept = true;
    write_json(&app.paths.workflow_templates_file(), &store).unwrap();
    assert!(
        apply(&app, &pending, &ready())
            .unwrap_err()
            .to_string()
            .contains("preview-stale")
    );
}

#[test]
fn setup_path_repair_preserves_instance_and_failed_diagnostic_save_can_retry() {
    let (_temp, app, candidate, _) = fixture();
    let mut repair = candidate.clone();
    repair.expected_fingerprint = Some(fingerprint(&candidate.config).unwrap());
    repair.config.executable_path = Some(std::env::current_exe().unwrap().to_string_lossy().into());
    let settings = register_success(
        &app.paths,
        &repair,
        &diagnostic(&repair),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&settings.agents.as_ref().unwrap()[&repair.id]).unwrap(),
        serde_json::to_value(&repair.config).unwrap()
    );
    assert!(
        register_success(
            &app.paths,
            &repair,
            &diagnostic(&repair),
            &AtomicBool::new(false)
        )
        .is_err(),
        "old captured baseline may not overwrite a later path change"
    );
    let new = SetupCandidate {
        id: "retry-local".parse().unwrap(),
        config: candidate.config,
        expected_fingerprint: None,
    };
    let path = app.paths.agent_diagnostics_file();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(
        register_success(&app.paths, &new, &diagnostic(&new), &AtomicBool::new(false)).is_err()
    );
    std::fs::remove_dir(&path).unwrap();
    let settings =
        register_success(&app.paths, &new, &diagnostic(&new), &AtomicBool::new(false)).unwrap();
    assert_eq!(settings.agents.unwrap().len(), 2);
}

#[test]
fn setup_upgrade_initializes_only_new_slots_and_preserves_deliberately_unbound_slots() {
    let (_temp, app, _, selection) = fixture();
    let settings = apply(&app, &request(&app, selection), &ready()).unwrap();
    let mut store = app.read_workflow_templates().unwrap();
    let template = &mut store.templates[0];
    let removed = template
        .workflow
        .nodes
        .iter()
        .find_map(|node| match node {
            crate::dsl::NodeDsl::Worker(worker) => worker.execution_slot_id.clone(),
            _ => None,
        })
        .unwrap();
    // Simulate an older built-in definition without one of today's slots.
    template.workflow.nodes.retain(|node| !matches!(node, crate::dsl::NodeDsl::Worker(worker) if worker.execution_slot_id.as_deref() == Some(&removed)));
    template.model_bindings.bindings.clear();
    let template_id = template.id.clone();
    write_json(&app.paths.workflow_templates_file(), &store).unwrap();
    let upgraded = App::with_config_and_path_config(
        app.paths.repo_root.clone(),
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    upgraded.initialize_workflow_templates().unwrap();
    let store = upgraded.read_workflow_templates().unwrap();
    let bindings = &store
        .templates
        .iter()
        .find(|item| item.id == template_id)
        .unwrap()
        .model_bindings
        .bindings;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].execution_slot_id, removed);
    assert_eq!(bindings[0].model_id.as_deref(), Some("test-model"));
}

#[test]
fn setup_custom_candidate_paging_is_bounded_and_does_not_launch_adapters() {
    let (_temp, app, candidate, _) = fixture();
    let mut settings = app.load_settings().unwrap();
    for index in 0..110 {
        settings.agents.as_mut().unwrap().insert(
            format!("custom-{index:03}").parse().unwrap(),
            candidate.config.clone(),
        );
    }
    let (first, cursor) = candidates(&settings, None).unwrap();
    assert_eq!(
        first.len(),
        crate::agent_catalog::builtin_agent_catalog().agents.len() + CUSTOM_AGENT_BATCH_SIZE
    );
    let (second, cursor) = candidates(&settings, cursor.as_deref()).unwrap();
    assert_eq!(second.len(), CUSTOM_AGENT_BATCH_SIZE);
    let (last, cursor) = candidates(&settings, cursor.as_deref()).unwrap();
    assert_eq!(last.len(), 10);
    assert!(cursor.is_none());
}

#[test]
fn setup_reuses_only_matching_healthy_configured_agent_diagnostics() {
    let (_temp, app, candidate, _) = fixture();
    let settings = app.load_settings().unwrap();
    let (batch, _) = candidates(&settings, None).unwrap();
    let configured = batch.iter().find(|item| item.id == candidate.id).unwrap();
    let path = app.paths.agent_diagnostics_file();
    let persisted_before = std::fs::read(&path).unwrap();
    let mut diagnostics: BTreeMap<crate::config::ManagedAgentId, ProviderDiagnosticSnapshot> =
        read_json(&path).unwrap();

    assert!(matches!(
        candidate_detection(configured, &diagnostics),
        CandidateDetection::Reuse(_)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), persisted_before);

    diagnostics.get_mut(&candidate.id).unwrap().available = false;
    assert!(matches!(
        candidate_detection(configured, &diagnostics),
        CandidateDetection::Unavailable(_)
    ));
    diagnostics.get_mut(&candidate.id).unwrap().available = true;
    diagnostics
        .get_mut(&candidate.id)
        .unwrap()
        .config_fingerprint = Some("stale".into());
    assert!(matches!(
        candidate_detection(configured, &diagnostics),
        CandidateDetection::Probe
    ));
    diagnostics.remove(&candidate.id);
    assert!(matches!(
        candidate_detection(configured, &diagnostics),
        CandidateDetection::Unavailable(_)
    ));

    let (fresh, _) = candidates(&crate::config::SettingsConfig::default(), None).unwrap();
    let unconfigured = fresh.iter().find(|item| item.id == candidate.id).unwrap();
    assert!(matches!(
        candidate_detection(unconfigured, &BTreeMap::new()),
        CandidateDetection::Probe
    ));
}

fn local_program(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = directory.join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    });
    std::fs::write(
        &path,
        if cfg!(windows) {
            "@echo off\r\n"
        } else {
            "#!/bin/sh\n"
        },
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    path
}

#[test]
fn setup_fresh_install_discovers_local_agents_without_saved_instances_or_npx_cache() {
    let temp = tempfile::tempdir().unwrap();
    let settings = crate::config::SettingsConfig::default();
    let (batch, _) = candidates(&settings, None).unwrap();
    assert!(settings.agents.as_ref().is_none_or(BTreeMap::is_empty));
    for (id, command, args) in [
        ("claude-acp", "claude-agent-acp", vec![]),
        ("codex-acp", "codex-acp", vec![]),
        ("gemini", "gemini", vec!["--acp"]),
        ("codebuddy-code", "codebuddy", vec!["--acp"]),
        ("qwen-code", "qwen", vec!["--acp", "--experimental-skills"]),
        ("pi-acp", "pi-acp", vec![]),
    ] {
        let executable = local_program(temp.path(), command);
        let mut candidate = batch
            .iter()
            .find(|candidate| candidate.id.as_str() == id)
            .unwrap()
            .clone();
        candidate
            .config
            .adapter
            .env
            .insert("PATH".into(), temp.path().to_string_lossy().into());
        let adapter = prepared(&candidate, temp.path())
            .unwrap()
            .unwrap_or_else(|| {
                panic!("fresh install must discover local {command} without adding it first")
            });
        assert_eq!(std::path::Path::new(&adapter.command), executable);
        assert_eq!(
            adapter.args, args,
            "{id} must receive ACP arguments, not npx package arguments"
        );
        let runtime = candidate.config.executable_adapter(&candidate.id).unwrap();
        assert_eq!(
            runtime.command, adapter.command,
            "normal execution must use the discovered executable"
        );
        assert_eq!(runtime.args, adapter.args);
    }
}

#[test]
fn setup_fresh_install_prepares_an_adapter_for_an_installed_agent_cli() {
    let temp = tempfile::tempdir().unwrap();
    let npx = local_program(temp.path(), "npx");
    // Resolve only this empty cache; do not inspect or populate the host npm cache.
    let npm = local_program(temp.path(), "npm");
    let cache = temp.path().join("empty-cache");
    let output = if cfg!(windows) {
        format!("@echo off\r\necho {}\r\n", cache.display())
    } else {
        format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", cache.display())
    };
    std::fs::write(npm, output).unwrap();
    std::fs::write(
        temp.path()
            .join(if cfg!(windows) { "node.exe" } else { "node" }),
        "fixture",
    )
    .unwrap();
    local_program(temp.path(), "claude");
    let (batch, _) = candidates(&crate::config::SettingsConfig::default(), None).unwrap();
    let mut candidate = batch
        .into_iter()
        .find(|candidate| candidate.id.as_str() == "claude-acp")
        .unwrap();
    candidate
        .config
        .adapter
        .env
        .insert("PATH".into(), temp.path().to_string_lossy().into());
    let PreparedAdapter::Unavailable(reason) =
        prepared_adapter(&candidate, temp.path(), false).unwrap()
    else {
        panic!("an npx adapter must not be prepared when npx cannot start");
    };
    assert_eq!(
        reason,
        SetupUnavailableReason::RuntimeRequired {
            tool: super::environment::EnvironmentTool::Npx
        }
    );
    let adapter = prepared(&candidate, temp.path())
        .unwrap()
        .expect("a local Agent CLI must enter ACP preparation on a fresh Gold Band install");
    assert_eq!(
        std::path::Path::new(&adapter.command),
        dunce::canonicalize(npx).unwrap()
    );
    assert_eq!(adapter.args, candidate.config.adapter.args);
    assert!(
        !cache.exists(),
        "discovery must not install anything before the controlled Doctor run"
    );
}

#[test]
fn setup_fresh_install_discovers_standard_npm_cache_without_a_global_cli() {
    use sha2::{Digest, Sha512};

    let temp = tempfile::tempdir().unwrap();
    local_program(temp.path(), "npx");
    let npm = local_program(temp.path(), "npm");
    let cache = temp.path().join("cache with spaces");
    let npm_source = if cfg!(windows) {
        format!("@echo off\r\necho {}\r\n", cache.display())
    } else {
        format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", cache.display())
    };
    std::fs::write(npm, npm_source).unwrap();
    let node = temp
        .path()
        .join(if cfg!(windows) { "node.exe" } else { "node" });
    std::fs::write(&node, "fixture").unwrap();
    let settings = crate::config::SettingsConfig::default();
    let (batch, _) = candidates(&settings, None).unwrap();
    let mut candidate = batch
        .into_iter()
        .find(|candidate| candidate.id.as_str() == "codebuddy-code")
        .unwrap();
    candidate
        .config
        .adapter
        .env
        .insert("PATH".into(), temp.path().to_string_lossy().into());
    let package_spec = candidate
        .config
        .adapter
        .args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .unwrap();
    let (name, version) = package_spec.rsplit_once('@').unwrap();
    let hash: String = Sha512::digest(package_spec.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let cache_root = cache.join("_npx").join(hash);
    let package_root = cache_root.join("node_modules").join(name);
    std::fs::create_dir_all(package_root.join("bin")).unwrap();
    std::fs::write(
        cache_root.join("package.json"),
        serde_json::json!({
            "dependencies": {(name): format!("^{version}")}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        cache_root.join("node_modules/.package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 3,
            "packages": {(format!("node_modules/{name}")): {"version": version}}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(package_root.join("package.json"), serde_json::json!({
        "name": name, "version": version,
        "bin": {"codebuddy-code": "bin/codebuddy", "codebuddy": "bin/codebuddy", "cbc": "bin/codebuddy"}
    }).to_string()).unwrap();
    let script = package_root.join("bin/codebuddy");
    std::fs::write(&script, "#!/usr/bin/env node\n").unwrap();
    let adapter = prepared(&candidate, temp.path()).unwrap().expect(
        "a locally cached Agent must be discovered before it has a saved instance or global CLI",
    );
    assert_eq!(std::path::Path::new(&adapter.command), node);
    assert_eq!(
        adapter.args,
        [script.to_string_lossy().into_owned(), "--acp".into()]
    );
    assert!(settings.agents.as_ref().is_none_or(BTreeMap::is_empty));
}

#[test]
fn setup_fresh_install_registers_all_detected_agents_before_selecting_a_default() {
    let temp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_owned()).unwrap();
    let paths = crate::storage::GoldBandPaths::new_with_path_config(root.clone(), PATH_CONFIG);
    let settings = initialization::initialize(&paths).unwrap();
    assert!(settings.agents.as_ref().unwrap().is_empty());
    let (batch, _) = candidates(&settings, None).unwrap();
    let executable = std::env::current_exe().unwrap();
    for command in ["opencode", "kimi"] {
        let program = local_program(temp.path(), command);
        let source = if cfg!(windows) {
            format!(
                "@echo off\r\n\"{}\" --ignored --exact acp::client::tests::doctor_adapter_fixture --nocapture\r\n",
                executable.display()
            )
        } else {
            format!(
                "#!/bin/sh\nexec '{}' --ignored --exact acp::client::tests::doctor_adapter_fixture --nocapture\n",
                executable.to_string_lossy().replace('\'', "'\\''")
            )
        };
        std::fs::write(program, source).unwrap();
    }
    for mut candidate in batch
        .into_iter()
        .filter(|candidate| matches!(candidate.id.as_str(), "opencode" | "kimi"))
    {
        candidate
            .config
            .adapter
            .env
            .insert("PATH".into(), temp.path().to_string_lossy().into());
        candidate.config.adapter.env.insert(
            "GOLD_BAND_DOCTOR_FIXTURE".into(),
            "model-config-onboarding".into(),
        );
        let cwd = IsolatedProbeDirectory::new().unwrap();
        let adapter = prepared(&candidate, cwd.0.as_std_path()).unwrap().unwrap();
        let diagnostic_dir = root.join(format!("doctor-{}", candidate.id.as_str()));
        let probe = crate::acp::client::doctor_prepared_with_control(
            &candidate.id,
            &adapter,
            cwd.0.clone(),
            diagnostic_dir.clone(),
            std::sync::Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let snapshot = ProviderDiagnosticSnapshot {
            capabilities: Some(probe.capabilities),
            ..diagnostic(&candidate)
        };
        for _ in 0..2 {
            register_success(&paths, &candidate, &snapshot, &AtomicBool::new(false)).unwrap();
        }
        assert!(!diagnostic_dir.join("provider.pid").exists());
        let methods = std::fs::read_to_string(cwd.0.join("fixture-methods")).unwrap();
        assert!(methods.contains("session/new"));
        assert!(!methods.contains("session/prompt"));
    }
    let settings = load_settings_file(&paths.user_settings_file()).unwrap();
    assert_eq!(settings.agents.as_ref().unwrap().len(), 2);
    assert!(settings.default_execution_profile.is_none());
    assert!(matches!(
        settings.onboarding,
        Some(OnboardingState::Pending { .. })
    ));
    let app = App::with_config_and_path_config(
        root,
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    assert_eq!(
        app.managed_agents()
            .keys()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["kimi", "opencode"]
    );
    assert_eq!(app.provider_diagnostics().len(), 2);
    assert!(
        app.provider_diagnostics()
            .values()
            .all(|diagnostic| diagnostic.available)
    );
}

#[test]
#[ignore = "requires explicitly selected installed and authenticated Agents; performs no inference"]
fn setup_installed_agents_acceptance() {
    let selected = std::env::var("GOLD_BAND_ONBOARDING_ACCEPTANCE_AGENTS")
        .expect("set GOLD_BAND_ONBOARDING_ACCEPTANCE_AGENTS to comma-separated catalog IDs");
    let expected: std::collections::BTreeSet<_> = selected
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    assert!(!expected.is_empty());
    let temp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_owned()).unwrap();
    let paths = crate::storage::GoldBandPaths::new_with_path_config(root.clone(), PATH_CONFIG);
    let settings = initialization::initialize(&paths).unwrap();
    assert!(settings.agents.as_ref().unwrap().is_empty());
    let (batch, _) = candidates(&settings, None).unwrap();
    let batch: Vec<_> = batch
        .into_iter()
        .filter(|candidate| expected.contains(candidate.id.as_str()))
        .collect();
    assert_eq!(
        batch.len(),
        expected.len(),
        "select only supported catalog IDs"
    );
    for candidate in batch {
        let cwd = IsolatedProbeDirectory::new().unwrap();
        let adapter = prepared(&candidate, cwd.0.as_std_path())
            .unwrap()
            .unwrap_or_else(|| panic!("{} was not discovered", candidate.id.as_str()));
        assert!(
            !crate::npx_cache::is_npx(&adapter),
            "acceptance must not download an adapter"
        );
        let diagnostic_dir = paths.doctor_acp_dir(&candidate.id);
        println!("local acceptance: {} discovered", candidate.id.as_str());
        let probe = crate::acp::client::doctor_prepared_with_control(
            &candidate.id,
            &adapter,
            cwd.0.clone(),
            diagnostic_dir.clone(),
            std::sync::Arc::new(AtomicBool::new(false)),
        )
        .unwrap_or_else(|error| {
            panic!(
                "{}: {}",
                candidate.id.as_str(),
                crate::acp::client::doctor_diagnostic_error(&error).code
            )
        });
        let snapshot = ProviderDiagnosticSnapshot {
            config_fingerprint: Some(candidate.config.diagnostic_fingerprint().unwrap()),
            available: true,
            error: None,
            checked_at: chrono::Utc::now().to_rfc3339(),
            capabilities: Some(probe.capabilities),
        };
        for _ in 0..2 {
            register_success(&paths, &candidate, &snapshot, &AtomicBool::new(false)).unwrap();
        }
        assert!(!diagnostic_dir.join("provider.pid").exists());
        println!(
            "local acceptance: {} verified and registered",
            candidate.id.as_str()
        );
    }
    let settings = load_settings_file(&paths.user_settings_file()).unwrap();
    assert!(settings.default_execution_profile.is_none());
    assert!(matches!(
        settings.onboarding,
        Some(OnboardingState::Pending { .. })
    ));
    let app = App::with_config_and_path_config(
        root,
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    assert_eq!(
        app.managed_agents()
            .keys()
            .map(|id| id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert_eq!(app.provider_diagnostics().len(), expected.len());
    assert!(
        app.provider_diagnostics()
            .values()
            .all(|diagnostic| diagnostic.available)
    );
}

#[test]
fn clean_setup_preview_builds_templates_in_memory_without_writing_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_owned()).unwrap();
    let paths = crate::storage::GoldBandPaths::new_with_path_config(root.clone(), PATH_CONFIG);
    let settings = initialization::initialize(&paths).unwrap();
    initialization::skip_git(&paths).unwrap();
    let app = App::with_config_and_path_config(
        root.clone(),
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    let candidate = SetupCandidate {
        id: "claude-acp".parse().unwrap(),
        config: ManagedAgentConfig::from_catalog(
            crate::agent_catalog::builtin_agent("claude-acp").unwrap(),
        ),
        expected_fingerprint: None,
    };
    let settings = register_success(
        &app.paths,
        &candidate,
        &diagnostic(&candidate),
        &AtomicBool::new(false),
    )
    .unwrap();
    let app = App::with_config_and_path_config(
        root,
        RuntimeConfig::default().apply_settings(&settings),
        PATH_CONFIG,
    );
    let settings_before = std::fs::read(app.paths.user_settings_file()).unwrap();
    let templates = app.paths.workflow_templates_file();
    let profiles = app.paths.user_profiles_dir();
    assert!(!templates.exists());
    assert!(!profiles.exists());

    let selection = DefaultSetupSelection {
        profile: DefaultExecutionProfile {
            agent_id: candidate.id,
            model_selection: ModelSelection::Explicit {
                model_id: "test-model".into(),
            },
            config_options: BTreeMap::new(),
            model_bound_overrides: BTreeMap::new(),
        },
        permission_mode_id: Some("default".into()),
    };
    let preview = preview(&app, &selection).unwrap();

    assert_eq!(
        preview.templates.len(),
        app.built_in_workflow_template_ids().len()
    );
    assert!(!templates.exists());
    assert!(!profiles.exists());
    assert_eq!(
        settings_before,
        std::fs::read(app.paths.user_settings_file()).unwrap()
    );

    let request = request(&app, selection);
    apply(&app, &request, &ready()).unwrap();
    assert!(templates.exists());
    assert!(matches!(
        app.load_settings().unwrap().onboarding,
        Some(OnboardingState::Completed { .. })
    ));
}

#[test]
#[ignore = "production-build workload: run with --release --ignored --nocapture"]
fn setup_large_template_storage_budget() {
    let (_temp, app, _, selection) = fixture();
    let mut store = app.read_workflow_templates().unwrap();
    for index in 0..1000 {
        let mut custom = store.templates[0].clone();
        custom.id = format!("user-{index}");
        custom.name = "User template ".repeat(600);
        custom.is_built_in = false;
        store.templates.push(custom);
    }
    write_json(&app.paths.workflow_templates_file(), &store).unwrap();
    let bytes = std::fs::metadata(app.paths.workflow_templates_file())
        .unwrap()
        .len();
    let start = std::time::Instant::now();
    let request = request(&app, selection);
    let preview_ms = start.elapsed().as_millis();
    let start = std::time::Instant::now();
    apply(&app, &request, &ready()).unwrap();
    let apply_ms = start.elapsed().as_millis();
    println!(
        "onboarding storage: bytes={bytes}, user_templates=1000, preview_ms={preview_ms}, apply_ms={apply_ms}"
    );
    assert_eq!(
        app.read_workflow_templates().unwrap().templates.len(),
        app.built_in_workflow_template_ids().len() + 1000
    );
    assert!(
        preview_ms <= 1000,
        "preview budget exceeded: {preview_ms}ms"
    );
    assert!(apply_ms <= 1000, "apply budget exceeded: {apply_ms}ms");
}
