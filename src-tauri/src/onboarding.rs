use crate::{
    commands::{CommandResult, command_error, emit_agent_registry_updated},
    state::{DesktopState, for_each_diagnostic_agent},
    view_models::{ManagedAgentVm, managed_agent_vm},
};
use gold_band::{
    config::{ManagedAgentId, ProviderDiagnosticSnapshot},
    onboarding::{
        defaults::{ApplyDefaultSetup, DefaultSetupPreview, DefaultSetupSelection},
        discovery::{
            CandidateDetection, IsolatedProbeDirectory, PreparedAdapter, SetupCandidate,
            SetupLaunchKind, SetupUnavailableReason, candidate_detection, candidates, launch_kind,
            prepared_adapter,
        },
        environment::{
            EnvironmentCheck, EnvironmentPolicy, EnvironmentReport, LocalEnvironmentRunner,
            check_environment, environment_policy,
        },
        model::{DefaultExecutionProfile, OnboardingState, SetupError, fingerprint},
    },
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Manager, State, ipc::Channel};

#[derive(Default)]
pub struct SetupController {
    active: Mutex<Option<Arc<SetupRun>>>,
    environment: Mutex<Option<SetupEnvironment>>,
}

struct SetupEnvironment {
    request_id: String,
    cancelled: Arc<AtomicBool>,
    report: Option<EnvironmentReport>,
}

pub struct SetupRun {
    pub generation: String,
    pub cancelled: Arc<AtomicBool>,
    npx_launchable: bool,
    suppressed: Mutex<BTreeSet<ManagedAgentId>>,
    channel: Channel<SetupEvent>,
}

impl SetupController {
    fn begin_environment(&self, request_id: String) -> anyhow::Result<Arc<AtomicBool>> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut environment = self
            .environment
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?;
        if let Some(previous) = environment.replace(SetupEnvironment {
            request_id,
            cancelled: cancelled.clone(),
            report: None,
        }) {
            previous.cancelled.store(true, Ordering::Release);
        }
        drop(environment);
        if let Some(run) = self
            .active
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?
            .as_ref()
        {
            run.cancelled.store(true, Ordering::Release);
        }
        Ok(cancelled)
    }

    fn complete_environment(
        &self,
        request_id: &str,
        report: EnvironmentReport,
    ) -> anyhow::Result<()> {
        let mut environment = self
            .environment
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?;
        if let Some(current) = environment.as_mut().filter(|current| {
            current.request_id == request_id && !current.cancelled.load(Ordering::Acquire)
        }) {
            current.report = Some(report);
        }
        Ok(())
    }

    fn checked_environment(&self) -> anyhow::Result<EnvironmentReport> {
        let environment = self
            .environment
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?;
        let report = environment
            .as_ref()
            .filter(|current| !current.cancelled.load(Ordering::Acquire))
            .and_then(|current| current.report.as_ref())
            .ok_or_else(|| SetupError::new("setup.environment-check-required"))?;
        Ok(report.clone())
    }

    fn cancel_environment(&self, request_id: &str) -> anyhow::Result<()> {
        let mut environment = self
            .environment
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?;
        if environment
            .as_ref()
            .is_some_and(|current| current.request_id == request_id)
            && let Some(current) = environment.take()
        {
            current.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn current(&self, generation: &str) -> anyhow::Result<Arc<SetupRun>> {
        self.active
            .lock()
            .map_err(|_| SetupError::new("setup.probe-failed"))?
            .as_ref()
            .filter(|run| run.generation == generation && !run.cancelled.load(Ordering::Acquire))
            .cloned()
            .ok_or_else(|| SetupError::new("setup.probe-cancelled").into())
    }

    pub fn invalidate_agent(&self, id: &ManagedAgentId) {
        if let Ok(active) = self.active.lock()
            && let Some(run) = active.as_ref()
        {
            if let Ok(mut suppressed) = run.suppressed.lock() {
                suppressed.insert(id.clone());
            }
            publish(
                run,
                &run.channel,
                SetupEvent::Withdrawn {
                    generation: run.generation.clone(),
                    agent_id: id.as_str().into(),
                },
            );
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(mut active) = self.active.lock()
            && let Some(run) = active.take()
        {
            run.cancelled.store(true, Ordering::Release);
        }
        if let Ok(mut environment) = self.environment.lock()
            && let Some(current) = environment.take()
        {
            current.cancelled.store(true, Ordering::Release);
        }
    }
}

impl SetupRun {
    fn allows(&self, id: &ManagedAgentId) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self
                .suppressed
                .lock()
                .is_ok_and(|suppressed| !suppressed.contains(id))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub platform: &'static str,
    pub state: OnboardingState,
    pub environment_policy: EnvironmentPolicy,
    pub git_minimum_version: &'static str,
    pub default_execution_profile: Option<DefaultExecutionProfile>,
    pub language: gold_band::config::DesktopLanguage,
    pub appearance: gold_band::config::AppearancePreference,
}

/// Only selection capabilities cross the setup channel; launch environment and
/// credential-bearing Agent metadata stay in the Agent management domain.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupAgentVm {
    pub agent_type: String,
    pub display_name: String,
    pub icon_key: String,
    pub supported_models: Option<Vec<crate::view_models::AcpModeVm>>,
    pub supported_modes: Option<Vec<crate::view_models::AcpModeVm>>,
    pub config_options: Option<Vec<crate::view_models::AcpSelectConfigOptionVm>>,
    pub model_bound_catalogs:
        Option<BTreeMap<String, Vec<crate::view_models::AcpSelectConfigOptionVm>>>,
}

impl From<ManagedAgentVm> for SetupAgentVm {
    fn from(agent: ManagedAgentVm) -> Self {
        Self {
            agent_type: agent.agent_type,
            display_name: agent.display_name,
            icon_key: agent.icon_key,
            supported_models: agent.supported_models,
            supported_modes: agent.supported_modes,
            config_options: agent.config_options,
            model_bound_catalogs: agent.model_bound_catalogs,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SetupEvent {
    Checking {
        generation: String,
        agent_id: String,
        label: String,
    },
    Ready {
        generation: String,
        agent: SetupAgentVm,
    },
    Unavailable {
        generation: String,
        agent_id: String,
        label: String,
        icon_key: String,
        reason: SetupUnavailableReason,
    },
    Withdrawn {
        generation: String,
        agent_id: String,
    },
    Finished {
        generation: String,
        next_cursor: Option<String>,
    },
    Failed {
        generation: String,
        code: String,
        params: serde_json::Value,
    },
}

fn publish(run: &SetupRun, channel: &Channel<SetupEvent>, event: SetupEvent) {
    if !run.cancelled.load(Ordering::Acquire) && channel.send(event).is_err() {
        run.cancelled.store(true, Ordering::Release);
    }
}

#[tauri::command]
pub fn get_onboarding_status(state: State<'_, DesktopState>) -> CommandResult<SetupStatus> {
    let context = state.context().map_err(command_error)?;
    let paths = gold_band::storage::GoldBandPaths::new(context.repo_root);
    let settings = gold_band::storage::load_settings_file(&paths.user_settings_file())
        .map_err(command_error)?;
    Ok(SetupStatus {
        platform: crate::view_models::DESKTOP_PLATFORM,
        state: settings
            .onboarding
            .unwrap_or(OnboardingState::ExistingInstall),
        environment_policy: environment_policy().clone(),
        git_minimum_version: gold_band::git::MINIMUM_SUPPORTED_GIT_VERSION,
        default_execution_profile: settings.default_execution_profile,
        language: context.config.desktop_language,
        appearance: context.config.appearance,
    })
}

#[tauri::command]
pub async fn check_setup_environment(
    controller: State<'_, SetupController>,
    state: State<'_, DesktopState>,
    request_id: String,
    on_checked: Channel<EnvironmentCheck>,
) -> CommandResult<EnvironmentReport> {
    let cancelled = {
        let _commit = state
            .agent_config_diagnostic_commit_guard()
            .map_err(command_error)?;
        controller
            .begin_environment(request_id.clone())
            .map_err(command_error)?
    };
    let report = tauri::async_runtime::spawn_blocking(move || {
        check_environment(
            &LocalEnvironmentRunner::default(),
            environment_policy(),
            &cancelled,
            |check| {
                if on_checked.send(check.clone()).is_err() {
                    cancelled.store(true, Ordering::Release);
                }
            },
        )
    })
    .await
    .map_err(|error| command_error(error.into()))?;
    controller
        .complete_environment(&request_id, report.clone())
        .map_err(command_error)?;
    Ok(report)
}

#[tauri::command]
pub fn cancel_setup_environment(
    controller: State<'_, SetupController>,
    request_id: String,
) -> CommandResult<()> {
    controller
        .cancel_environment(&request_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn skip_setup_git(state: State<'_, DesktopState>) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let settings =
        gold_band::onboarding::initialization::skip_git(&app.paths).map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)
}

#[tauri::command]
pub async fn defer_setup(handle: AppHandle) -> CommandResult<()> {
    // Configuration writes and runtime startup touch disk; keep them off the IPC thread.
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<DesktopState>();
        // Deferring is only valid once the current environment check has completed.
        handle
            .state::<SetupController>()
            .checked_environment()
            .map_err(command_error)?;
        let app = state.app().map_err(command_error)?;
        let settings =
            gold_band::onboarding::initialization::defer(&app.paths).map_err(command_error)?;
        state
            .update_settings_config(&settings)
            .map_err(command_error)?;
        crate::start_business_runtime(handle);
        Ok(())
    })
    .await
    .map_err(|error| command_error(error.into()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::onboarding::environment::{
        EnvironmentError, EnvironmentStatus, EnvironmentTool,
    };

    #[test]
    fn legacy_doctor_models_do_not_become_setup_model_choices() {
        let agent_id = "claude-acp".parse().unwrap();
        let config = gold_band::config::ManagedAgentConfig::from_catalog(
            gold_band::agent_catalog::builtin_agent("claude-acp").unwrap(),
        );
        let diagnostic = ProviderDiagnosticSnapshot {
            available: true,
            checked_at: "2026-11-02T00:00:00Z".into(),
            error: None,
            config_fingerprint: Some(config.diagnostic_fingerprint().unwrap()),
            capabilities: Some(serde_json::json!({
                "models": {
                    "currentModelId": "legacy-model",
                    "availableModels": [{"modelId": "legacy-model", "name": "Legacy model"}]
                },
                "modes": {
                    "currentModeId": "default",
                    "availableModes": [{"id": "default", "name": "Default"}]
                }
            })),
        };
        let agent = SetupAgentVm::from(managed_agent_vm(&agent_id, &config, Some(&diagnostic)));
        let payload = serde_json::to_value(agent).unwrap();
        assert!(payload["supportedModels"].is_null());
        assert!(payload["configOptions"].is_null());
        assert_eq!(payload["supportedModes"][0]["id"], "default");
    }

    #[test]
    fn unavailable_agent_event_carries_identity_and_a_structured_reason() {
        let payload = serde_json::to_value(SetupEvent::Unavailable {
            generation: "test-generation".into(),
            agent_id: "claude-acp".into(),
            label: "Claude".into(),
            icon_key: "claude".into(),
            reason: SetupUnavailableReason::RuntimeRequired {
                tool: EnvironmentTool::Npx,
            },
        })
        .unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "type": "unavailable",
                "generation": "test-generation",
                "agentId": "claude-acp",
                "label": "Claude",
                "iconKey": "claude",
                "reason": {"kind": "runtime-required", "tool": "npx"}
            })
        );

        let payload = serde_json::to_value(SetupEvent::Unavailable {
            generation: "test-generation".into(),
            agent_id: "qwen-code".into(),
            label: "Qwen Code".into(),
            icon_key: "qwen-code".into(),
            reason: SetupUnavailableReason::DoctorFailed {
                launch: SetupLaunchKind::Npx,
                error: Some(gold_band::config::DiagnosticError {
                    code: "acp.session-request-failed".into(),
                    params: serde_json::json!({"method": "session/new"}),
                    raw: Some(serde_json::json!({"message": "Authentication required"})),
                }),
            },
        })
        .unwrap();
        assert_eq!(
            payload["reason"],
            serde_json::json!({
                "kind": "doctor-failed",
                "launch": "npx",
                "error": {
                    "code": "acp.session-request-failed",
                    "params": {"method": "session/new"},
                    "raw": {"message": "Authentication required"}
                }
            })
        );

        assert_eq!(
            serde_json::to_value(SetupEvent::Withdrawn {
                generation: "test-generation".into(),
                agent_id: "codex-acp".into(),
            })
            .unwrap(),
            serde_json::json!({
                "type": "withdrawn",
                "generation": "test-generation",
                "agentId": "codex-acp"
            })
        );
    }

    #[test]
    fn environment_failures_do_not_block_detection_and_keep_the_report() {
        let report = EnvironmentReport::from_checks(vec![
            EnvironmentCheck {
                tool: EnvironmentTool::Node,
                status: EnvironmentStatus::Timeout,
                version: None,
                executable_path: Some("fixture with spaces/node.exe".into()),
                node_executable_path: None,
                node_version: None,
                error: Some(EnvironmentError {
                    code: "setup.probe-timeout".into(),
                    params: serde_json::json!({"tool": "node"}),
                }),
            },
            EnvironmentCheck {
                tool: EnvironmentTool::Npx,
                status: EnvironmentStatus::Missing,
                version: None,
                executable_path: None,
                node_executable_path: None,
                node_version: None,
                error: Some(EnvironmentError {
                    code: "setup.runtime-missing".into(),
                    params: serde_json::json!({"tool": "npx"}),
                }),
            },
        ]);
        assert!(!report.npx_launchable);
        let controller = SetupController::default();
        controller.begin_environment("limited".into()).unwrap();
        controller
            .complete_environment("limited", report.clone())
            .unwrap();

        let checked = controller.checked_environment().unwrap();

        assert_eq!(
            serde_json::to_value(checked).unwrap(),
            serde_json::to_value(report).unwrap()
        );
    }

    #[test]
    fn completed_environment_is_reused_until_explicit_refresh_or_exit() {
        let controller = SetupController::default();
        assert!(
            controller
                .checked_environment()
                .unwrap_err()
                .to_string()
                .contains("environment-check-required")
        );
        let first = controller.begin_environment("first".into()).unwrap();
        assert!(controller.checked_environment().is_err());
        let report = EnvironmentReport {
            npx_launchable: true,
            checks: vec![],
        };
        controller
            .complete_environment("first", report.clone())
            .unwrap();
        for _ in 0..3 {
            assert_eq!(
                serde_json::to_value(controller.checked_environment().unwrap()).unwrap(),
                serde_json::to_value(&report).unwrap()
            );
        }
        let second = controller.begin_environment("second".into()).unwrap();
        assert!(first.load(Ordering::Acquire));
        assert!(controller.checked_environment().is_err());
        controller
            .complete_environment("first", report.clone())
            .unwrap();
        assert!(controller.checked_environment().is_err());
        controller.complete_environment("second", report).unwrap();
        controller.cancel_environment("first").unwrap();
        assert!(controller.checked_environment().unwrap().npx_launchable);
        controller.cancel_environment("second").unwrap();
        assert!(second.load(Ordering::Acquire));
        assert!(controller.checked_environment().is_err());
    }

    #[test]
    fn cancelled_environment_cannot_be_restored_by_a_late_result() {
        let controller = SetupController::default();
        let cancelled = controller.begin_environment("cancelled".into()).unwrap();
        controller.cancel_environment("cancelled").unwrap();
        controller
            .complete_environment(
                "cancelled",
                EnvironmentReport {
                    npx_launchable: true,
                    checks: vec![],
                },
            )
            .unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        assert!(controller.checked_environment().is_err());
    }

    #[test]
    fn explicit_refresh_replaces_a_previous_pass_with_its_failed_report() {
        let controller = SetupController::default();
        controller.begin_environment("passed".into()).unwrap();
        controller
            .complete_environment(
                "passed",
                EnvironmentReport {
                    npx_launchable: true,
                    checks: vec![],
                },
            )
            .unwrap();
        controller.begin_environment("failed".into()).unwrap();
        controller
            .complete_environment(
                "failed",
                EnvironmentReport {
                    npx_launchable: false,
                    checks: vec![],
                },
            )
            .unwrap();
        assert!(!controller.checked_environment().unwrap().npx_launchable);
        controller.cancel_all();
        assert!(
            controller
                .checked_environment()
                .unwrap_err()
                .to_string()
                .contains("environment-check-required")
        );
    }
}

fn probe_candidate(
    handle: &AppHandle,
    run: &Arc<SetupRun>,
    candidate: &SetupCandidate,
    on_event: &Channel<SetupEvent>,
) -> anyhow::Result<()> {
    let state = handle.state::<DesktopState>();
    let _run_guard = state.setup_diagnostic_guard(&candidate.id, &run.cancelled)?;
    if !run.allows(&candidate.id) {
        return Err(SetupError::new("setup.probe-cancelled").into());
    }
    let cwd = IsolatedProbeDirectory::new()?;
    let adapter = match prepared_adapter(candidate, cwd.0.as_std_path(), run.npx_launchable)? {
        PreparedAdapter::Ready(adapter) => adapter,
        PreparedAdapter::Unavailable(reason) => {
            publish_candidate_unavailable(run, candidate, on_event, reason);
            return Ok(());
        }
    };
    publish(
        run,
        on_event,
        SetupEvent::Checking {
            generation: run.generation.clone(),
            agent_id: candidate.id.as_str().into(),
            label: candidate.config.adapter.display_name.clone(),
        },
    );
    let app = state.app()?;
    let result = gold_band::acp::client::doctor_prepared_with_control(
        &candidate.id,
        &adapter,
        cwd.0.clone(),
        app.paths.doctor_acp_dir(&candidate.id),
        run.cancelled.clone(),
    );
    let _commit = state.agent_config_diagnostic_commit_guard()?;
    if !run.allows(&candidate.id) {
        return Err(SetupError::new("setup.probe-cancelled").into());
    }
    let probe = result?;
    let diagnostic = ProviderDiagnosticSnapshot {
        config_fingerprint: Some(candidate.config.diagnostic_fingerprint()?),
        available: true,
        error: None,
        checked_at: chrono::Utc::now().to_rfc3339(),
        capabilities: Some(probe.capabilities),
    };
    state.persist_setup_success(candidate, diagnostic, &run.cancelled)?;
    let app = state.app()?;
    let diagnostics = state.agent_diagnostics()?;
    let config = app
        .managed_agents()
        .get(&candidate.id)
        .ok_or_else(|| SetupError::new("setup.agent-unavailable"))?;
    publish(
        run,
        on_event,
        SetupEvent::Ready {
            generation: run.generation.clone(),
            agent: managed_agent_vm(&candidate.id, config, diagnostics.get(&candidate.id)).into(),
        },
    );
    drop(_commit);
    emit_agent_registry_updated(handle, &candidate.id);
    Ok(())
}

fn publish_candidate_unavailable(
    run: &SetupRun,
    candidate: &SetupCandidate,
    channel: &Channel<SetupEvent>,
    reason: SetupUnavailableReason,
) {
    if !run.allows(&candidate.id) {
        return;
    }
    publish(
        run,
        channel,
        SetupEvent::Unavailable {
            generation: run.generation.clone(),
            agent_id: candidate.id.as_str().into(),
            label: candidate.config.adapter.display_name.clone(),
            icon_key: candidate.config.icon.clone(),
            reason,
        },
    );
}

fn doctor_failed(
    candidate: &SetupCandidate,
    error: Option<gold_band::config::DiagnosticError>,
) -> SetupUnavailableReason {
    SetupUnavailableReason::DoctorFailed {
        launch: launch_kind(candidate).unwrap_or(SetupLaunchKind::Local),
        error,
    }
}

fn publish_candidate_failure(
    run: &SetupRun,
    candidate: &SetupCandidate,
    channel: &Channel<SetupEvent>,
    error: &anyhow::Error,
) {
    let error = gold_band::acp::client::doctor_diagnostic_error(error);
    publish_candidate_unavailable(
        run,
        candidate,
        channel,
        doctor_failed(candidate, Some(error)),
    );
}

fn publish_candidate_withdrawn(
    run: &SetupRun,
    candidate: &SetupCandidate,
    channel: &Channel<SetupEvent>,
) {
    if run.allows(&candidate.id) {
        publish(
            run,
            channel,
            SetupEvent::Withdrawn {
                generation: run.generation.clone(),
                agent_id: candidate.id.as_str().into(),
            },
        );
    }
}

fn publish_configured_candidate(
    handle: &AppHandle,
    run: &SetupRun,
    candidate: &SetupCandidate,
    channel: &Channel<SetupEvent>,
) -> anyhow::Result<bool> {
    if candidate.expected_fingerprint.is_none() {
        return Ok(false);
    }
    let state = handle.state::<DesktopState>();
    let _commit = state.agent_config_diagnostic_commit_guard()?;
    if !run.allows(&candidate.id) {
        return Ok(true);
    }
    let app = state.app()?;
    let diagnostics = state.agent_diagnostics()?;
    let current = app.managed_agents().get(&candidate.id);
    let current_matches = current.map(fingerprint).transpose()?.as_deref()
        == candidate.expected_fingerprint.as_deref();
    if !current_matches {
        publish_candidate_withdrawn(run, candidate, channel);
        return Ok(true);
    }
    match candidate_detection(candidate, &diagnostics) {
        CandidateDetection::Reuse(diagnostic) => {
            let Some(config) = current else {
                publish_candidate_withdrawn(run, candidate, channel);
                return Ok(true);
            };
            publish(
                run,
                channel,
                SetupEvent::Ready {
                    generation: run.generation.clone(),
                    agent: managed_agent_vm(&candidate.id, config, Some(diagnostic)).into(),
                },
            );
            Ok(true)
        }
        CandidateDetection::Probe => Ok(false),
        CandidateDetection::Unavailable(diagnostic) => {
            publish_candidate_unavailable(
                run,
                candidate,
                channel,
                doctor_failed(candidate, diagnostic.and_then(|d| d.error.clone())),
            );
            Ok(true)
        }
    }
}

#[tauri::command]
pub fn start_setup_agent_detection(
    handle: AppHandle,
    controller: State<'_, SetupController>,
    after: Option<String>,
    on_event: Channel<SetupEvent>,
) -> CommandResult<String> {
    let run = {
        let state = handle.state::<DesktopState>();
        let _guard = state
            .agent_config_diagnostic_commit_guard()
            .map_err(command_error)?;
        let environment = controller.checked_environment().map_err(command_error)?;
        let run = Arc::new(SetupRun {
            generation: uuid::Uuid::new_v4().to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
            npx_launchable: environment.npx_launchable,
            suppressed: Mutex::new(BTreeSet::new()),
            channel: on_event.clone(),
        });
        let mut active = controller
            .active
            .lock()
            .map_err(|_| command_error(SetupError::new("setup.probe-failed").into()))?;
        if let Some(previous) = active.replace(run.clone()) {
            previous.cancelled.store(true, Ordering::Release);
        }
        state.setup_active.store(true, Ordering::Release);
        run
    };
    let generation = run.generation.clone();
    let rollback_handle = handle.clone();
    let worker = std::thread::Builder::new()
        .name("onboarding-detection".into())
        .spawn(move || {
            let work = || -> anyhow::Result<()> {
                let state = handle.state::<DesktopState>();
                if run.cancelled.load(Ordering::Acquire) {
                    return Ok(());
                }
                let app = state.app()?;
                let (batch, next_cursor) = candidates(&app.load_settings()?, after.as_deref())?;
                let batch: BTreeMap<_, _> = batch
                    .into_iter()
                    .map(|candidate| (candidate.id.clone(), candidate))
                    .collect();
                let ids: Vec<_> = batch.keys().cloned().collect();
                for_each_diagnostic_agent(&ids, |id| {
                    let candidate = &batch[id];
                    match publish_configured_candidate(&handle, &run, candidate, &on_event) {
                        Ok(true) => {}
                        Ok(false) => {
                            if let Err(error) = probe_candidate(&handle, &run, candidate, &on_event)
                            {
                                publish_candidate_failure(&run, candidate, &on_event, &error);
                            }
                        }
                        Err(error) => publish_candidate_failure(&run, candidate, &on_event, &error),
                    }
                });
                publish(
                    &run,
                    &on_event,
                    SetupEvent::Finished {
                        generation: run.generation.clone(),
                        next_cursor,
                    },
                );
                Ok(())
            };
            if let Err(error) = work() {
                let error = command_error(error);
                publish(
                    &run,
                    &on_event,
                    SetupEvent::Failed {
                        generation: run.generation.clone(),
                        code: error.code,
                        params: error.params,
                    },
                );
                run.cancelled.store(true, Ordering::Release);
            }
            if run.cancelled.load(Ordering::Acquire) {
                let controller = handle.state::<SetupController>();
                let _ = cancel_setup_detection(handle.clone(), controller, run.generation.clone());
            }
        });
    if let Err(error) = worker {
        cancel_setup_detection(rollback_handle, controller, generation.clone())?;
        return Err(command_error(error.into()));
    }
    Ok(generation)
}

#[tauri::command]
pub fn cancel_setup_detection(
    handle: AppHandle,
    controller: State<'_, SetupController>,
    generation: String,
) -> CommandResult<()> {
    let state = handle.state::<DesktopState>();
    let _guard = state
        .agent_config_diagnostic_commit_guard()
        .map_err(command_error)?;
    let mut active = controller
        .active
        .lock()
        .map_err(|_| command_error(SetupError::new("setup.probe-failed").into()))?;
    if active
        .as_ref()
        .is_some_and(|run| run.generation == generation)
    {
        if let Some(run) = active.take() {
            run.cancelled.store(true, Ordering::Release);
        }
        state.setup_active.store(false, Ordering::Release);
    }
    Ok(())
}

#[tauri::command]
pub async fn probe_setup_model(
    handle: AppHandle,
    generation: String,
    agent_id: String,
    model_id: String,
) -> CommandResult<SetupAgentVm> {
    tauri::async_runtime::spawn_blocking(move || {
        let run = handle
            .state::<SetupController>()
            .current(&generation)
            .map_err(command_error)?;
        let state = handle.state::<DesktopState>();
        let id: ManagedAgentId = agent_id.parse().map_err(command_error)?;
        let _guard = state
            .setup_diagnostic_guard(&id, &run.cancelled)
            .map_err(command_error)?;
        let app = state.app().map_err(command_error)?;
        let settings = app.load_settings().map_err(command_error)?;
        let config = settings
            .agents
            .as_ref()
            .and_then(|agents| agents.get(&id))
            .cloned()
            .ok_or_else(|| command_error(SetupError::new("setup.agent-unavailable").into()))?;
        let expected = fingerprint(&config).map_err(command_error)?;
        let diagnostic_fingerprint = config.diagnostic_fingerprint().map_err(command_error)?;
        let diagnostics = state.agent_diagnostics().map_err(command_error)?;
        if !diagnostics.get(&id).is_some_and(|d| {
            d.available && d.config_fingerprint.as_deref() == Some(&diagnostic_fingerprint)
        }) {
            return Err(command_error(
                SetupError::new("setup.agent-unavailable").into(),
            ));
        }
        let candidate = SetupCandidate {
            id: id.clone(),
            config,
            expected_fingerprint: Some(expected.clone()),
        };
        let cwd = IsolatedProbeDirectory::new().map_err(command_error)?;
        let PreparedAdapter::Ready(adapter) =
            prepared_adapter(&candidate, cwd.0.as_std_path(), run.npx_launchable)
                .map_err(command_error)?
        else {
            return Err(command_error(
                SetupError::new("setup.agent-preparation-required").into(),
            ));
        };
        let options = gold_band::acp::client::probe_prepared_model_with_control(
            &id,
            &adapter,
            cwd.0.clone(),
            app.paths.doctor_acp_dir(&id),
            &model_id,
            run.cancelled.clone(),
        )
        .map_err(command_error)?;
        let _commit = state
            .agent_config_diagnostic_commit_guard()
            .map_err(command_error)?;
        if !run.allows(&id) {
            return Err(command_error(
                SetupError::new("setup.probe-cancelled").into(),
            ));
        }
        let app = state.app().map_err(command_error)?;
        let config = app
            .managed_agents()
            .get(&id)
            .ok_or_else(|| command_error(SetupError::new("setup.agent-unavailable").into()))?;
        if fingerprint(config).map_err(command_error)? != expected {
            return Err(command_error(
                SetupError::new("setup.configuration-conflict").into(),
            ));
        }
        state
            .record_setup_model(&id, &options)
            .map_err(command_error)?;
        let diagnostics = state.agent_diagnostics().map_err(command_error)?;
        Ok(managed_agent_vm(&id, config, diagnostics.get(&id)).into())
    })
    .await
    .map_err(|error| command_error(error.into()))?
}

#[tauri::command]
pub async fn preview_default_setup(
    handle: AppHandle,
    selection: DefaultSetupSelection,
) -> CommandResult<DefaultSetupPreview> {
    tauri::async_runtime::spawn_blocking(move || {
        let app = handle
            .state::<DesktopState>()
            .app()
            .map_err(command_error)?;
        gold_band::onboarding::defaults::preview(&app, &selection).map_err(command_error)
    })
    .await
    .map_err(|error| command_error(error.into()))?
}

#[tauri::command]
pub async fn apply_default_setup(
    handle: AppHandle,
    request: ApplyDefaultSetup,
) -> CommandResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<DesktopState>();
        let app = state.app().map_err(command_error)?;
        if let Some(settings) = gold_band::onboarding::defaults::applied_settings(&app, &request)
            .map_err(command_error)?
        {
            state
                .update_settings_config(&settings)
                .map_err(command_error)?;
            crate::start_business_runtime(handle);
            return Ok(());
        }
        let _commit = state
            .agent_config_diagnostic_commit_guard()
            .map_err(command_error)?;
        let environment = handle
            .state::<SetupController>()
            .checked_environment()
            .map_err(command_error)?;
        let app = state.app().map_err(command_error)?;
        let settings = gold_band::onboarding::defaults::apply(&app, &request, &environment)
            .map_err(command_error)?;
        state
            .update_settings_config(&settings)
            .map_err(command_error)?;
        drop(_commit);
        crate::start_business_runtime(handle);
        Ok(())
    })
    .await
    .map_err(|error| command_error(error.into()))?
}

#[tauri::command]
pub fn save_setup_preferences(
    state: State<'_, DesktopState>,
    language: gold_band::config::DesktopLanguage,
    appearance: gold_band::config::AppearancePreference,
) -> CommandResult<()> {
    let app = state.app().map_err(command_error)?;
    let settings = gold_band::storage::configuration::with_configuration(
        &app.paths.user_gold_band_dir(),
        || {
            let mut settings = app.load_settings()?;
            settings.desktop_language = Some(language);
            settings.appearance = Some(appearance);
            gold_band::storage::save_settings_file(&app.paths.user_settings_file(), &settings)?;
            app.load_settings()
        },
    )
    .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)
}
