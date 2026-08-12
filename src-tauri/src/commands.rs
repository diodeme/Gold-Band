use gold_band::acp::client;
use gold_band::acp::commands::{AcpCommandCatalog, parse_available_commands};
use gold_band::acp::elicitation::{
    ElicitationAction, cancel_pending_elicitation_requests, write_elicitation_response,
};
use gold_band::acp::events::{AcpUiEvent, compact_live_conversation_event, current_timestamp};
use gold_band::acp::permission::{
    PendingPermissionState, cancel_pending_permission_requests,
    write_permission_response_if_pending,
};
use gold_band::acp::prompt_queue::{
    AUTO_DISPATCH_USER_PRIORITY_GRACE_MS, AutoClaimResult, PromptQueueError,
    auto_dispatch_is_suspended, claim_next_for_auto_dispatch, claim_queued_prompt,
    clear_auto_dispatch_reply_batch, clear_auto_dispatch_suspension, complete_accepted_prompt,
    delete_queued_prompt, enqueue_prompt, load_prompt_queue, mark_user_priority,
    record_auto_dispatch_reply_completion, release_queued_prompt, request_auto_dispatch_suspension,
    settle_dispatching_prompts, suspend_auto_dispatch, update_queued_prompt,
};
use gold_band::acp::turn_files::{
    CHANGE_SET_NOT_FOUND, TurnFileChangeSet, TurnFileStore, VERSION_NOT_FOUND,
};
use gold_band::app::{
    AcpPromptLifecycleEvent, AcpTurnBatchProgress, AcpTurnOutcome, App, AutoTemplate,
    AutoTemplateStore, CreateTaskInput, ImportProfilesInput, ImportProfilesResult,
    ProfileCommandError, ProfileEntry, ProfileInput, ProfileList, RuntimeInterventionKind,
    RuntimeLifecycleEvent, WorkflowTemplateStore,
};
use gold_band::domain::{NodeOutcome, PauseReason, RunOutcome, RunStatus, SessionMode};
use gold_band::dsl::{AiDynamicAgentStrategy, NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::dynamic::{DynamicGraphState, DynamicNodeStatus, DynamicRunStatus};
use gold_band::runtime::{NodeState, RunState, WorkerRefState};
use gold_band::skill::SkillCommandError;
use gold_band::storage::read_json;
use gold_band::storage::sqlite::{self, AttemptIndexContext};
use std::path::{Component, Path, PathBuf};
use std::{
    collections::BTreeSet,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use camino::Utf8PathBuf;
use gold_band::config::{
    AcpAdapterConfig, ConversationAutoConfig, DEFAULT_CUSTOM_AGENT_ICON, DesktopFontPreference,
    DesktopLanguage, DesktopThemePreference, ManagedAgentConfig, ManagedAgentId,
};
use gold_band::observability::set_runtime_log_level;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::avatar::{
    AvatarKind, AvatarPreferencesVm, AvatarShape, SaveDesktopAvatarInput, clear_avatar,
    load_avatar_preferences, save_avatar_image, save_avatar_shape, select_recent_avatar,
};
use crate::conversation_workspace::workspace_entry_for_project;
use crate::i18n::Translator;
use crate::metrics::{MetricsSettingsVm, metrics_settings, normalize_metrics_base_url};
use crate::state::{DesktopState, NotificationAttentionInput, UpdateBadgeSeenTarget};
use crate::updater::{
    UpdateStatusVm, UpdaterSettingsVm, check_update,
    download_and_install_update as run_download_and_install_update, install_pending_file,
    normalize_updater_url_override, updater_settings,
};
use crate::view_models::{
    AcpActivityDetailQueryInput, AcpActivityDetailVm, AcpRawFramePageVm, AcpRawFrameQueryInput,
    AcpSessionQueryInput, AcpSessionVm, AcpToolDetailQueryInput, AcpToolDetailVm, AgentRegistryVm,
    AppBootstrapVm, ContentVm, LocalClaudeStatusVm, LogPageVm, LogQueryInput, McpServerVm,
    PreferencesVm, RoundDetailVm, RoundSelectionInput, RunDetailVm, RunSummaryVm, SkillContentVm,
    SkillListVm, SkillMetaVm, SyncStatusEntryVm, TaskDetailVm, TaskListVm, UpdateBadgeStateVm,
    WorkflowVm, acp_activity_detail_vm_for_attempt, acp_raw_frame_page_vm, acp_session_vm,
    acp_tool_detail_vm_for_attempt, agent_registry_vm, bootstrap_vm, dynamic_acp_session_vm,
    log_page_vm, mcp_server_list_vm, preferences_vm, round_detail_vm, run_detail_vm,
    run_summary_vm, skill_content_vm, skill_list_vm, skill_meta_vm, task_detail_vm, task_list_vm,
    workflow_vm,
};
use crate::view_models_conversation::{
    ConversationAttemptLifecycleVm, ConversationTaskActivityVm, conversation_attempt_lifecycle_vm,
    conversation_is_orchestrated, conversation_run_mode, conversation_task_activity_from_prompt,
};

const ACP_SESSION_EVENT: &str = "gold-band://acp-session-updated";
const AGENT_REGISTRY_UPDATED_EVENT: &str = "gold-band://agent-registry-updated";
const AGENT_COMMANDS_UPDATED_EVENT: &str = "gold-band://agent-commands-updated";
const CONVERSATION_RUN_STATE_EVENT: &str = "gold-band://conversation-run-state-updated";
const PERMISSION_REQUESTED_DEDUP_SUFFIX: &str = "permission-requested";
const ELICITATION_REQUESTED_DEDUP_SUFFIX: &str = "elicitation-requested";
const QUEUED_PROMPT_ID_PREFIX: &str = "turn-queued-";

pub type CommandResult<T> = Result<T, CommandErrorVm>;

pub(crate) async fn spawn_blocking_command<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[derive(Debug, Clone)]
struct AttemptLocator {
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
}

impl AttemptLocator {
    fn new(
        task_id: String,
        run_id: String,
        round_id: String,
        node_id: String,
        attempt_id: String,
        outer_node_id: Option<String>,
        outer_attempt_id: Option<String>,
    ) -> Self {
        let has_outer = outer_node_id.is_some() && outer_attempt_id.is_some();
        Self {
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id: has_outer.then(|| outer_node_id.unwrap()),
            outer_attempt_id: has_outer.then(|| outer_attempt_id.unwrap()),
        }
    }

    fn outer_node_id(&self) -> Option<&str> {
        self.outer_node_id.as_deref()
    }

    fn outer_attempt_id(&self) -> Option<&str> {
        self.outer_attempt_id.as_deref()
    }

    fn runtime_node_id(&self) -> &str {
        self.outer_node_id().unwrap_or(&self.node_id)
    }

    fn runtime_attempt_id(&self) -> &str {
        self.outer_attempt_id().unwrap_or(&self.attempt_id)
    }

    fn matches_run_current(&self, run: &RunState) -> bool {
        run.current_round.as_deref() == Some(self.round_id.as_str())
            && run.current_node.as_deref() == Some(self.runtime_node_id())
            && run.current_attempt.as_deref() == Some(self.runtime_attempt_id())
    }

    fn attempt_dir(&self, app: &gold_band::app::App) -> Utf8PathBuf {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (self.outer_node_id(), self.outer_attempt_id())
        {
            app.paths.dynamic_node_attempt_dir(
                &self.task_id,
                &self.run_id,
                &self.round_id,
                outer_node_id,
                outer_attempt_id,
                &self.node_id,
                &self.attempt_id,
            )
        } else {
            app.paths.attempt_dir(
                &self.task_id,
                &self.run_id,
                &self.round_id,
                &self.node_id,
                &self.attempt_id,
            )
        }
    }
}

fn resolve_acp_attempt_dir(
    app: &gold_band::app::App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> Utf8PathBuf {
    AttemptLocator::new(
        task_id.to_string(),
        run_id.to_string(),
        round_id.to_string(),
        node_id.to_string(),
        attempt_id.to_string(),
        outer_node_id.map(str::to_string),
        outer_attempt_id.map(str::to_string),
    )
    .attempt_dir(app)
}

fn lifecycle_for_locator(
    app: &App,
    locator: &AttemptLocator,
) -> Option<ConversationAttemptLifecycleVm> {
    conversation_attempt_lifecycle_vm(
        app,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    )
    .ok()
}

fn runtime_continue_started_lifecycle_for_locator(
    app: &App,
    locator: &AttemptLocator,
) -> Option<ConversationAttemptLifecycleVm> {
    lifecycle_for_locator(app, locator)
}

fn current_attempt_manual_check_pending(
    app: &App,
    locator: &AttemptLocator,
    run: &RunState,
) -> CommandResult<bool> {
    if !locator.matches_run_current(run) || locator.outer_node_id().is_some() {
        return Ok(false);
    }
    let node_path = app.paths.node_file(
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
    );
    read_json::<NodeState>(&node_path)
        .map(|node| node.manual_check_pending)
        .map_err(command_error)
}

fn acp_attempt_was_cancelled(attempt_dir: &Utf8PathBuf) -> bool {
    [
        attempt_dir.join("acp.snapshot.json"),
        attempt_dir.join("acp.session.json"),
    ]
    .iter()
    .any(|path| {
        read_json::<serde_json::Value>(path)
            .ok()
            .and_then(|value| {
                value
                    .get("stopReason")
                    .or_else(|| value.get("stop_reason"))
                    .and_then(|reason| reason.as_str().map(str::to_string))
                    .or_else(|| {
                        value
                            .get("status")
                            .and_then(|status| status.as_str().map(str::to_string))
                    })
            })
            .is_some_and(|value| value.eq_ignore_ascii_case("cancelled"))
    })
}

fn dynamic_leaf_runtime_continue_required(
    app: &App,
    locator: &AttemptLocator,
    run: &RunState,
) -> CommandResult<bool> {
    if run.status == RunStatus::Paused
        && !run
            .pause_reason
            .is_some_and(PauseReason::allows_explicit_runtime_continue)
    {
        return Ok(false);
    }
    let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    else {
        return Ok(false);
    };
    let dynamic_graph = read_json::<DynamicGraphState>(&app.paths.dynamic_graph_file(
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        outer_node_id,
        outer_attempt_id,
    ))
    .map_err(command_error)?;
    if dynamic_graph.run.status == DynamicRunStatus::Paused
        && !dynamic_graph
            .run
            .pause_reason
            .is_some_and(PauseReason::allows_explicit_runtime_continue)
    {
        return Ok(false);
    }
    let dynamic_node = dynamic_graph
        .nodes
        .iter()
        .find(|node| node.id == locator.node_id)
        .ok_or_else(|| {
            command_error(anyhow::anyhow!(
                "dynamic node `{}` not found",
                locator.node_id
            ))
        })?;
    if dynamic_node.status == DynamicNodeStatus::Paused && dynamic_node.outcome.is_none() {
        return Ok(dynamic_node
            .pause_reason
            .is_some_and(PauseReason::allows_explicit_runtime_continue));
    }
    let stale_resumable_parent = (run.status == RunStatus::Paused
        && run
            .pause_reason
            .is_some_and(PauseReason::allows_explicit_runtime_continue))
        || (dynamic_graph.run.status == DynamicRunStatus::Paused
            && dynamic_graph
                .run
                .pause_reason
                .is_some_and(PauseReason::allows_explicit_runtime_continue));
    let stale_resumable_leaf = stale_resumable_parent
        && matches!(
            dynamic_node.status,
            DynamicNodeStatus::Ready | DynamicNodeStatus::Running
        )
        && dynamic_node.outcome.is_none()
        && acp_attempt_was_cancelled(&locator.attempt_dir(app));
    Ok(stale_resumable_leaf)
}

fn runtime_continue_required(
    app: &App,
    locator: &AttemptLocator,
    run: &RunState,
    manual_check_pending: bool,
) -> CommandResult<bool> {
    if !conversation_is_orchestrated(app, &locator.task_id) || manual_check_pending {
        return Ok(false);
    }
    if locator.matches_run_current(run) && locator.outer_node_id().is_some() {
        return dynamic_leaf_runtime_continue_required(app, locator, run);
    }
    if run.status == RunStatus::Paused
        && gold_band::app::is_run_continuable(run)
        && run
            .pause_reason
            .is_some_and(PauseReason::allows_explicit_runtime_continue)
        && locator.matches_run_current(run)
    {
        return Ok(true);
    }
    Ok(false)
}

fn attempt_is_runtime_controlled(app: &App, locator: &AttemptLocator) -> CommandResult<bool> {
    if !conversation_is_orchestrated(app, &locator.task_id) {
        return Ok(false);
    }
    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        let node = read_json::<gold_band::dynamic::DynamicNodeState>(&app.paths.dynamic_node_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
        ))
        .map_err(command_error)?;
        return Ok(node.status == DynamicNodeStatus::Running);
    }
    let node = read_json::<NodeState>(&app.paths.node_file(
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
    ))
    .map_err(command_error)?;
    Ok(node.status == RunStatus::Running && !node.manual_check_pending)
}

fn ensure_conversation_prompt_available(app: &App, locator: &AttemptLocator) -> CommandResult<()> {
    if attempt_is_runtime_controlled(app, locator)? {
        return Err(CommandErrorVm::new(
            "runtime.conversation-not-available",
            serde_json::json!({
                "taskId": locator.task_id,
                "runId": locator.run_id,
                "roundId": locator.round_id,
                "nodeId": locator.node_id,
                "attemptId": locator.attempt_id,
            }),
        ));
    }
    Ok(())
}

fn acp_turn_outcome(run: &client::AcpPromptRun) -> AcpTurnOutcome {
    acp_turn_outcome_for_stop_reason(run.stop_reason.as_deref())
}

fn acp_turn_outcome_for_stop_reason(stop_reason: Option<&str>) -> AcpTurnOutcome {
    match stop_reason
        .map(|reason| reason.trim().to_ascii_lowercase().replace('_', "-"))
        .as_deref()
    {
        Some("cancelled" | "canceled") => AcpTurnOutcome::Cancelled,
        Some("interrupted") => AcpTurnOutcome::Failed,
        _ => AcpTurnOutcome::Completed,
    }
}

fn acp_turn_provider_id(app: &App, locator: &AttemptLocator) -> Option<String> {
    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        read_json::<gold_band::dynamic::DynamicNodeState>(&app.paths.dynamic_node_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
        ))
        .ok()
        .and_then(|node| node.provider)
    } else {
        read_json::<NodeState>(&app.paths.node_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        ))
        .ok()
        .and_then(|node| {
            node.resolved_config
                .get("provider")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
    }
}

fn acp_turn_agent_label(app: &App, locator: &AttemptLocator) -> String {
    let provider = acp_turn_provider_id(app, locator);
    provider
        .as_deref()
        .and_then(|provider| app.managed_agent(provider).ok())
        .map(|(_, config)| config.adapter.display_name.clone())
        .filter(|label| !label.trim().is_empty())
        .or(provider)
        .unwrap_or_else(|| locator.node_id.clone())
}

fn emit_acp_turn_finished(
    app: &App,
    locator: &AttemptLocator,
    turn_id: &str,
    agent_label: &str,
    outcome: AcpTurnOutcome,
    batch_progress: AcpTurnBatchProgress,
) {
    app.emit_lifecycle_event(RuntimeLifecycleEvent::AcpTurnFinished {
        event_id: gold_band::app::make_turn_dedup_key(
            &app.paths.project_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            turn_id,
        ),
        occurred_at: current_timestamp(),
        scheduled_occurrence_id: None,
        project_id: app.paths.project_id.clone(),
        task_id: locator.task_id.clone(),
        run_id: locator.run_id.clone(),
        round_id: locator.round_id.clone(),
        node_id: locator.node_id.clone(),
        attempt_id: locator.attempt_id.clone(),
        turn_id: turn_id.to_string(),
        agent_label: agent_label.to_string(),
        outcome,
        batch_progress,
        task_title: app
            .task_show(&locator.task_id)
            .ok()
            .and_then(|task| task.title),
    });
}

fn finish_acp_prompt_preflight<T>(
    app: &App,
    locator: &AttemptLocator,
    turn_id: &str,
    agent_label: &str,
    result: CommandResult<T>,
) -> CommandResult<T> {
    if result.is_err() && app.scheduled_occurrence_id().is_some() {
        emit_acp_turn_finished(
            app,
            locator,
            turn_id,
            agent_label,
            AcpTurnOutcome::Failed,
            AcpTurnBatchProgress::terminal(1),
        );
    }
    result
}

#[derive(Debug, Clone)]
struct DeferredTurnCompletion {
    turn_id: String,
    agent_label: String,
}

fn emit_deferred_turn_completion(
    app: &App,
    locator: &AttemptLocator,
    completion: Option<&DeferredTurnCompletion>,
    auto_dispatch_continues: bool,
) {
    if let Some(completion) = completion {
        let completed_reply_count = record_auto_dispatch_reply_completion(
            &locator.attempt_dir(app),
            auto_dispatch_continues,
        )
        .unwrap_or(1);
        emit_acp_turn_finished(
            app,
            locator,
            &completion.turn_id,
            &completion.agent_label,
            AcpTurnOutcome::Completed,
            AcpTurnBatchProgress {
                completed_reply_count,
                continues: auto_dispatch_continues,
            },
        );
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcpSessionUpdatedEventVm {
    branch_id: Option<String>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
    lifecycle: Option<ConversationAttemptLifecycleVm>,
    activity: Option<ConversationTaskActivityVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRunStateUpdatedEventVm {
    project_id: String,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    status: RunStatus,
    outcome: Option<RunOutcome>,
}

pub(crate) fn resolve_command_app(
    state: &DesktopState,
    project_id: Option<&str>,
) -> Result<App, CommandErrorVm> {
    match project_id {
        None => state.app().map_err(command_error),
        Some(pid) => {
            let global_app = state.app().map_err(command_error)?;
            let app_state = global_app.load_state().map_err(command_error)?;
            let (workspace_path, _) =
                workspace_entry_for_project(&app_state, pid).ok_or_else(|| {
                    CommandErrorVm::new(
                        "workspace.not-found",
                        serde_json::json!({ "projectId": pid }),
                    )
                })?;
            let context = state.context().map_err(command_error)?;
            Ok(global_app.with_repo_root(Utf8PathBuf::from(workspace_path), context.config))
        }
    }
}

pub(crate) fn register_lifecycle_subscribers(app: &App, app_handle: &AppHandle) {
    app.lifecycle_bus.subscribe_named(
        "desktop.metrics",
        crate::metrics::create_metrics_subscriber(app_handle.clone()),
    );
    app.lifecycle_bus.subscribe_named(
        "desktop.notifications",
        crate::notifications::create_intervention_notification_subscriber(
            app_handle.clone(),
            app.config.notification_auto_dismiss_target_secs,
        ),
    );
    app.lifecycle_bus.subscribe_named(
        "desktop.conversation-run-state",
        create_conversation_run_state_subscriber(app_handle.clone()),
    );
}

fn create_conversation_run_state_subscriber(
    app_handle: AppHandle,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
    Arc::new(move |event| {
        if let Some(payload) = conversation_run_state_update_for_event(event) {
            let _ = app_handle.emit(CONVERSATION_RUN_STATE_EVENT, payload);
        }
    })
}

fn conversation_run_state_update_for_event(
    event: RuntimeLifecycleEvent,
) -> Option<ConversationRunStateUpdatedEventVm> {
    match event {
        RuntimeLifecycleEvent::RunPaused {
            project_id,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            ..
        } => Some(ConversationRunStateUpdatedEventVm {
            project_id,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            status: RunStatus::Paused,
            outcome: None,
        }),
        RuntimeLifecycleEvent::RunCompleted {
            project_id,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outcome,
            ..
        } => Some(ConversationRunStateUpdatedEventVm {
            project_id,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            status: RunStatus::Completed,
            outcome: Some(outcome),
        }),
        _ => None,
    }
}

pub(crate) fn acp_live_update_emitter_for_app(
    app: &App,
    app_handle: AppHandle,
    project_id: Option<String>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext, AcpUiEvent) -> anyhow::Result<()> + Send + Sync>
{
    acp_live_update_emitter(
        app_handle,
        project_id,
        Some(app.clone_for_background()),
        Some(app.lifecycle_bus.clone()),
    )
}

fn resolve_command_app_with_emitters(
    app_handle: &AppHandle,
    state: &DesktopState,
    project_id: Option<&str>,
) -> Result<App, CommandErrorVm> {
    let app = resolve_command_app(state, project_id)?;
    let pid = project_id.map(|s| s.to_string());
    Ok(configure_conversation_runtime_callbacks(
        app,
        app_handle.clone(),
        pid,
    ))
}

pub(crate) fn configure_conversation_runtime_callbacks(
    app: App,
    app_handle: AppHandle,
    project_id: Option<String>,
) -> App {
    let bg_app = app.clone_for_background();
    let live_update = acp_live_update_emitter_for_app(&app, app_handle.clone(), project_id.clone());
    let prompt_turn_lifecycle = prompt_turn_lifecycle_callback(
        app_handle.clone(),
        app.clone_for_background(),
        project_id.clone(),
    );
    app.with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(app_handle, bg_app, project_id))
        .with_prompt_turn_lifecycle(prompt_turn_lifecycle)
}

fn prompt_turn_lifecycle_callback(
    app_handle: AppHandle,
    app: App,
    project_id: Option<String>,
) -> Arc<
    dyn Fn(gold_band::app::AcpLiveEventContext, AcpPromptLifecycleEvent) -> anyhow::Result<()>
        + Send
        + Sync,
> {
    Arc::new(move |context, event| {
        let locator = AttemptLocator::new(
            context.task_id,
            context.run_id,
            context.round_id,
            context.node_id,
            context.attempt_id,
            context.outer_node_id,
            context.outer_attempt_id,
        );
        match event {
            AcpPromptLifecycleEvent::Accepted { prompt_id } => {
                let _ = complete_accepted_prompt(&locator.attempt_dir(&app), &prompt_id);
            }
            AcpPromptLifecycleEvent::Finished {
                prompt_id,
                successful,
            } => {
                let completion = prompt_id.map(|turn_id| DeferredTurnCompletion {
                    turn_id,
                    agent_label: acp_turn_agent_label(&app, &locator),
                });
                schedule_direct_prompt_queue_drain(
                    app_handle.clone(),
                    project_id.clone(),
                    locator,
                    successful,
                    completion,
                );
            }
        }
        Ok(())
    })
}

fn schedule_direct_prompt_queue_drain(
    app_handle: AppHandle,
    project_id: Option<String>,
    locator: AttemptLocator,
    successful: bool,
    completed_turn: Option<DeferredTurnCompletion>,
) {
    let state = app_handle.state::<DesktopState>();
    let Ok(app) = resolve_command_app(state.inner(), project_id.as_deref()) else {
        return;
    };
    if conversation_run_mode(&app, &locator.task_id)
        != Some(gold_band::config::ConversationRunMode::Direct)
    {
        return;
    }
    let attempt_dir = locator.attempt_dir(&app);
    let queue = match settle_dispatching_prompts(&attempt_dir) {
        Ok(queue) => queue,
        Err(_) => {
            emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
            return;
        }
    };
    if !successful {
        let _ = clear_auto_dispatch_reply_batch(&attempt_dir);
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id,
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id,
            locator.outer_attempt_id,
            None,
        );
        return;
    }
    if queue.auto_dispatch_suspended || auto_dispatch_is_suspended(&attempt_dir) {
        emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
        return;
    }
    if !queue
        .items
        .iter()
        .any(|item| item.state == gold_band::acp::prompt_queue::QueuedPromptState::Queued)
    {
        emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
        return;
    }
    let expected_revision = queue.revision;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(AUTO_DISPATCH_USER_PRIORITY_GRACE_MS)).await;
        let state = app_handle.state::<DesktopState>();
        let Ok(app) = resolve_command_app(state.inner(), project_id.as_deref()) else {
            return;
        };
        let attempt_dir = locator.attempt_dir(&app);
        if client::prompt_activity(&attempt_dir).is_some() {
            emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
            return;
        }
        let claimed = match claim_next_for_auto_dispatch(&attempt_dir, expected_revision) {
            Ok(AutoClaimResult::Claimed(item)) => item,
            Ok(
                AutoClaimResult::Empty | AutoClaimResult::Preempted | AutoClaimResult::Suspended,
            )
            | Err(_) => {
                emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
                return;
            }
        };
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id.clone(),
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            None,
        );
        if auto_dispatch_is_suspended(&attempt_dir) {
            let _ = release_queued_prompt(&attempt_dir, &claimed.id);
            emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
            return;
        }
        emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), true);
        let queued_turn_app = app.clone_for_background().without_scheduled_turn_context();
        let _result = send_acp_prompt_with_app(
            app_handle.clone(),
            queued_turn_app,
            project_id.clone(),
            locator.task_id.clone(),
            locator.run_id.clone(),
            locator.round_id.clone(),
            locator.node_id.clone(),
            locator.attempt_id.clone(),
            claimed.content.clone(),
            Some(claimed.prompt_id.clone()),
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            (!claimed.attachment_paths.is_empty()).then_some(claimed.attachment_paths.clone()),
        )
        .await;
        let _ = settle_dispatching_prompts(&attempt_dir);
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id.clone(),
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            None,
        );
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorVm {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExitPreparationWarningVm {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExitPreparationVm {
    pub warnings: Vec<AppExitPreparationWarningVm>,
}

impl AppExitPreparationVm {
    fn record_warning(&mut self, code: &str, error: &dyn std::fmt::Display) {
        warn!(warning_code = code, %error, "application exit preparation step failed");
        self.warnings.push(AppExitPreparationWarningVm {
            code: code.to_string(),
            params: serde_json::json!({}),
        });
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptSubmitVm {
    pub kind: String,
    pub session: Option<AcpSessionVm>,
    pub run: Option<RunSummaryVm>,
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptQueueMutationVm {
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSessionStopVm {
    pub operation_id: String,
    pub status: String,
    pub kind: String,
    pub run: Option<RunSummaryVm>,
    pub session: Option<AcpSessionVm>,
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

impl CommandErrorVm {
    pub fn new(code: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            code: code.into(),
            params,
        }
    }
}

pub(crate) async fn prepare_app_exit_inner(
    app_handle: &AppHandle,
    state: &DesktopState,
) -> AppExitPreparationVm {
    let mut result = AppExitPreparationVm::default();

    // Stop the scheduler before stopping ACP/runtime sessions so no new
    // occurrence can acquire a lease while desktop cleanup is in progress.
    // `shutdown` waits for both the coordinator acknowledgement and task join.
    if let Ok(coordinator) = state.scheduler_coordinator()
        && let Err(error) = coordinator.shutdown().await
    {
        result.record_warning("app-exit.scheduler-shutdown-failed", &error);
    }

    match state.app() {
        Ok(runtime_app) => {
            match tauri::async_runtime::spawn_blocking(move || {
                runtime_app.stop_all_running_sessions().map(|_| ())
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => result.record_warning("app-exit.session-stop-failed", &error),
                Err(error) => result.record_warning("app-exit.session-stop-task-failed", &error),
            }
        }
        Err(error) => result.record_warning("app-exit.runtime-unavailable", &error),
    }

    if let Err(error) = state.cleanup_agent_diagnostic_processes() {
        result.record_warning("app-exit.diagnostic-cleanup-failed", &error);
    }

    if let Some(path) = state.take_pending_update()
        && let Err(error) = install_pending_file(&app_handle, &path).await
    {
        result.record_warning("app-exit.update-install-failed", &error);
    }

    result
}

fn ensure_no_active_acp_prompts_in_workspace(
    workspace_root: &camino::Utf8Path,
) -> CommandResult<()> {
    if gold_band::acp::client::has_active_prompts_in_workspace(workspace_root) {
        return Err(CommandErrorVm::new(
            "acp.active-prompt-blocks-config-save",
            serde_json::json!({ "workspaceRoot": workspace_root.as_str() }),
        ));
    }
    Ok(())
}

fn ensure_no_active_acp_prompts_for_provider(agent_id: &ManagedAgentId) -> CommandResult<()> {
    if gold_band::acp::client::has_active_prompts_in_provider(agent_id.as_str()) {
        return Err(CommandErrorVm::new(
            "acp.active-prompt-blocks-config-save",
            serde_json::json!({ "agentType": agent_id.as_str() }),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentInput {
    pub display_name: String,
    #[serde(default)]
    pub icon: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub primary_agent_dir: String,
    #[serde(default)]
    pub project_primary_agent_dir: Option<String>,
    #[serde(default)]
    pub compatible_agent_dirs: Vec<String>,
    #[serde(default)]
    pub external_session_sync_supported: bool,
    #[serde(default)]
    pub external_session_sync_enabled: bool,
}

impl ManagedAgentInput {
    fn into_config(
        self,
        system_prompt_delivery: gold_band::config::SystemPromptDelivery,
        default_icon: &str,
    ) -> CommandResult<ManagedAgentConfig> {
        let display_name = self.display_name.trim().to_string();
        if display_name.is_empty() {
            return Err(CommandErrorVm::new(
                "agent.display-name-required",
                serde_json::json!({}),
            ));
        }
        let command = self.command.trim().to_string();
        if command.is_empty() {
            return Err(CommandErrorVm::new(
                "agent.command-required",
                serde_json::json!({}),
            ));
        }
        let primary_agent_dir = self.primary_agent_dir.trim().to_string();
        let primary_agent_dir = (!primary_agent_dir.is_empty()).then_some(primary_agent_dir);
        let project_primary_agent_dir = self
            .project_primary_agent_dir
            .map(|directory| directory.trim().to_string());
        let mut compatible_agent_dirs = Vec::new();
        for directory in self.compatible_agent_dirs {
            let directory = directory.trim().to_string();
            if !directory.is_empty()
                && primary_agent_dir.as_deref() != Some(directory.as_str())
                && project_primary_agent_dir.as_deref() != Some(directory.as_str())
                && !compatible_agent_dirs.contains(&directory)
            {
                compatible_agent_dirs.push(directory);
            }
        }
        Ok(ManagedAgentConfig {
            adapter: AcpAdapterConfig {
                command,
                args: self.args,
                display_name,
                env: self.env,
            },
            icon: match self.icon.trim() {
                "" => default_icon.to_string(),
                value => value.to_string(),
            },
            primary_agent_dir,
            project_primary_agent_dir,
            compatible_agent_dirs,
            system_prompt_delivery,
            external_session_sync_supported: self.external_session_sync_supported,
            external_session_sync_enabled: self.external_session_sync_supported
                && self.external_session_sync_enabled,
        })
    }
}

fn system_prompt_delivery_for_new_agent(
    agent_id: &ManagedAgentId,
) -> gold_band::config::SystemPromptDelivery {
    gold_band::config::catalog_agent_default_config(agent_id.as_str())
        .map(|config| config.system_prompt_delivery)
        .unwrap_or_default()
}

fn default_icon_for_agent(agent_id: &ManagedAgentId) -> String {
    gold_band::config::catalog_agent_default_config(agent_id.as_str())
        .map(|config| config.icon)
        .unwrap_or_else(|| DEFAULT_CUSTOM_AGENT_ICON.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInputVm {
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub requirement_file_name: Option<String>,
    pub requirement_content: String,
    pub workflow: WorkflowDsl,
    pub workflow_template_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowInputVm {
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowTemplateInputVm {
    pub name: String,
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowTemplateInputVm {
    pub workflow: WorkflowDsl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAutoTemplateInputVm {
    pub name: String,
    pub config: ConversationAutoConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutoTemplateInputVm {
    pub name: String,
    pub config: ConversationAutoConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceAutoTemplatesInputVm {
    pub templates: Vec<AutoTemplate>,
}

#[tauri::command]
pub fn get_system_fonts() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let mut families = BTreeSet::new();
    for face in database.faces() {
        for (family, _) in &face.families {
            families.insert(family.clone());
        }
    }
    families.into_iter().collect()
}

#[tauri::command]
pub fn check_local_claude() -> LocalClaudeStatusVm {
    match gold_band::process::find_executable_in_path("claude") {
        Some(path) => LocalClaudeStatusVm {
            found: true,
            path: Some(path.to_string_lossy().into_owned()),
        },
        None => LocalClaudeStatusVm {
            found: false,
            path: None,
        },
    }
}

#[tauri::command]
pub fn get_app_bootstrap(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
) -> CommandResult<AppBootstrapVm> {
    let context = state.context().map_err(command_error)?;
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app_handle.package_info().version.to_string(),
        context.needs_workspace,
    ))
}

#[tauri::command]
pub async fn get_agent_registry(state: State<'_, DesktopState>) -> CommandResult<AgentRegistryVm> {
    let context = state.context().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        Ok(agent_registry_vm(&app, &diagnostics))
    })
    .await
}

#[tauri::command]
pub fn create_agent(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    agent_type: String,
    input: ManagedAgentInput,
) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    if app.managed_agents().contains_key(&agent_id) {
        return Err(CommandErrorVm::new(
            "agent.already-exists",
            serde_json::json!({ "agentType": agent_id.as_str() }),
        ));
    }
    let system_prompt_delivery = system_prompt_delivery_for_new_agent(&agent_id);
    let default_icon = default_icon_for_agent(&agent_id);
    ensure_no_active_acp_prompts_for_provider(&agent_id)?;
    gold_band::acp::client::close_provider_connections_bounded(agent_id.as_str())
        .map_err(command_error)?;
    let config_commit_guard = state
        .agent_config_diagnostic_commit_guard()
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let settings = app
        .save_managed_agent(
            agent_id.clone(),
            input.into_config(system_prompt_delivery, &default_icon)?,
        )
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    state
        .clear_agent_diagnostic(&agent_id)
        .map_err(command_error)?;
    drop(config_commit_guard);
    schedule_agent_diagnostic(&app_handle, agent_id);
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn update_agent(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    agent_type: String,
    input: ManagedAgentInput,
) -> CommandResult<AgentRegistryVm> {
    let app = state.app().map_err(command_error)?;
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    let system_prompt_delivery = app
        .managed_agents()
        .get(&agent_id)
        .map(|config| config.system_prompt_delivery)
        .ok_or_else(|| {
            CommandErrorVm::new(
                "agent.not-configured",
                serde_json::json!({ "agentType": agent_id.as_str() }),
            )
        })?;
    let default_icon = default_icon_for_agent(&agent_id);
    ensure_no_active_acp_prompts_for_provider(&agent_id)?;
    gold_band::acp::client::close_provider_connections_bounded(agent_id.as_str())
        .map_err(command_error)?;
    let config_commit_guard = state
        .agent_config_diagnostic_commit_guard()
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let settings = app
        .save_managed_agent(
            agent_id.clone(),
            input.into_config(system_prompt_delivery, &default_icon)?,
        )
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    state
        .clear_agent_diagnostic(&agent_id)
        .map_err(command_error)?;
    drop(config_commit_guard);
    schedule_agent_diagnostic(&app_handle, agent_id);
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub fn delete_agent(
    state: State<'_, DesktopState>,
    agent_type: String,
) -> CommandResult<AgentRegistryVm> {
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    ensure_no_active_acp_prompts_for_provider(&agent_id)?;
    gold_band::acp::client::close_provider_connections_bounded(agent_id.as_str())
        .map_err(command_error)?;
    let _config_commit_guard = state
        .agent_config_diagnostic_commit_guard()
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let settings = app.remove_managed_agent(&agent_id).map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    state
        .cancel_queued_agent_diagnostic(&agent_id)
        .map_err(command_error)?;
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

#[tauri::command]
pub async fn doctor_agent(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    agent_type: String,
) -> CommandResult<AgentRegistryVm> {
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    state
        .refresh_agent_diagnostic(&agent_id)
        .map_err(command_error)?;
    emit_agent_registry_updated(&app_handle);
    emit_agent_commands_updated(&app_handle, None);
    let app = state.app().map_err(command_error)?;
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    Ok(agent_registry_vm(&app, &diagnostics))
}

fn schedule_agent_diagnostic(app_handle: &AppHandle, agent_id: ManagedAgentId) {
    let state = app_handle.state::<DesktopState>();
    match state.queue_agent_diagnostic(&agent_id) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            warn!(agent_type = agent_id.as_str(), %error, "failed to queue agent diagnostic");
            return;
        }
    }

    let diagnostic_handle = app_handle.clone();
    let thread_name = format!("agent-doctor-{}", agent_id.as_str());
    let diagnostic_agent_id = agent_id.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let state = diagnostic_handle.state::<DesktopState>();
            if let Err(error) = state.run_queued_agent_diagnostic(&diagnostic_agent_id) {
                warn!(agent_type = diagnostic_agent_id.as_str(), %error, "automatic agent diagnostic failed");
            }
            emit_agent_registry_updated(&diagnostic_handle);
            emit_agent_commands_updated(&diagnostic_handle, None);
        })
    {
        let _ = state.cancel_queued_agent_diagnostic(&agent_id);
        warn!(agent_type = agent_id.as_str(), %error, "failed to start agent diagnostic thread");
    }
}

#[tauri::command]
pub fn get_agent_command_catalog(
    state: State<'_, DesktopState>,
    agent_type: String,
    workspace_path: String,
) -> CommandResult<Option<AcpCommandCatalog>> {
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    state
        .agent_command_catalog(&agent_id, &Utf8PathBuf::from(workspace_path))
        .map_err(command_error)
}

pub(crate) fn emit_agent_commands_updated(
    app_handle: &AppHandle,
    catalog: Option<&AcpCommandCatalog>,
) {
    let payload = catalog
        .map(|catalog| serde_json::to_value(catalog).unwrap_or_else(|_| serde_json::json!({})))
        .unwrap_or_else(|| serde_json::json!({ "refresh": true }));
    let _ = app_handle.emit(AGENT_COMMANDS_UPDATED_EVENT, payload);
}

pub(crate) fn emit_agent_registry_updated(app_handle: &AppHandle) {
    let _ = app_handle.emit(AGENT_REGISTRY_UPDATED_EVENT, ());
}

#[tauri::command]
pub fn get_task_list(state: State<'_, DesktopState>) -> CommandResult<TaskListVm> {
    let app = state.app().map_err(command_error)?;
    task_list_vm(&app).map_err(command_error)
}

#[tauri::command]
pub async fn get_profiles(state: State<'_, DesktopState>) -> CommandResult<ProfileList> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || context.app().profiles().map_err(command_error)).await
}

#[tauri::command]
pub fn get_profile(state: State<'_, DesktopState>, id: String) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.profile_show(&id).map_err(command_error)
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, DesktopState>,
    input: ProfileInput,
) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.create_profile(input).map_err(command_error)
}

#[tauri::command]
pub async fn import_profiles_from_folder(
    state: State<'_, DesktopState>,
    input: ImportProfilesInput,
) -> CommandResult<ImportProfilesResult> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        context
            .app()
            .import_profiles_from_folder(input)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, DesktopState>,
    id: String,
    input: ProfileInput,
) -> CommandResult<ProfileEntry> {
    let app = state.app().map_err(command_error)?;
    app.update_profile(&id, input).map_err(command_error)
}

#[tauri::command]
pub fn delete_profile(
    state: State<'_, DesktopState>,
    id: String,
    force: Option<bool>,
) -> CommandResult<ProfileList> {
    let app = state.app().map_err(|error| {
        CommandErrorVm::new(
            "app.unexpected",
            serde_json::json!({
                "message": format!("delete_profile `{}` failed before execution: {:#}", id, error),
            }),
        )
    })?;
    match app.delete_profile(&id, force.unwrap_or(false)) {
        Ok(list) => Ok(list),
        Err(error) => {
            if error.downcast_ref::<ProfileCommandError>().is_some() {
                Err(command_error(error))
            } else {
                Err(CommandErrorVm::new(
                    "app.unexpected",
                    serde_json::json!({
                        "message": format!("delete_profile `{}` failed: {:#}", id, error),
                    }),
                ))
            }
        }
    }
}

#[tauri::command]
pub async fn choose_workspace(
    app: AppHandle,
    state: State<'_, DesktopState>,
    path: String,
) -> CommandResult<AppBootstrapVm> {
    let repo_root = Utf8PathBuf::from_path_buf(std::path::PathBuf::from(&path))
        .map_err(|_| CommandErrorVm::new("workspace.path-invalid-utf8", serde_json::json!({})))?;
    info!(selected_repo_root = %repo_root, "workspace picker returned selection");
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    info!(
        active_repo_root = %context.repo_root,
        recent_workspace_count = context.recent_workspaces.len(),
        "workspace selection applied"
    );
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app.package_info().version.to_string(),
        false,
    ))
}

#[tauri::command]
pub fn select_recent_workspace(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    workspace: String,
) -> CommandResult<AppBootstrapVm> {
    info!(selected_repo_root = %workspace, "switching to recent workspace");
    let repo_root = Utf8PathBuf::from(workspace);
    let context = state.set_workspace(repo_root).map_err(command_error)?;
    info!(
        active_repo_root = %context.repo_root,
        recent_workspace_count = context.recent_workspaces.len(),
        "recent workspace selection applied"
    );
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app_handle.package_info().version.to_string(),
        false,
    ))
}

#[tauri::command]
pub fn remove_recent_workspace(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    workspace: String,
) -> CommandResult<AppBootstrapVm> {
    info!(workspace = %workspace, "removing recent workspace");
    let current_context = state.context().map_err(command_error)?;
    if workspace == current_context.repo_root.as_str() {
        return Err(CommandErrorVm::new(
            "workspace.recent-current-locked",
            serde_json::json!({ "workspace": workspace }),
        ));
    }
    if current_context.recent_workspaces.len() <= 1 {
        return Err(CommandErrorVm::new(
            "workspace.recent-minimum-required",
            serde_json::json!({ "workspace": workspace }),
        ));
    }
    let context = state
        .remove_recent_workspace(&workspace)
        .map_err(command_error)?;
    info!(
        recent_workspace_count = context.recent_workspaces.len(),
        "recent workspace removed"
    );
    let update_status = state.update_status().map_err(command_error)?;
    Ok(bootstrap_vm(
        &context.app(),
        context.recent_workspaces,
        update_status,
        app_handle.package_info().version.to_string(),
        context.needs_workspace,
    ))
}

#[tauri::command]
pub fn get_task_detail(
    state: State<'_, DesktopState>,
    task_id: String,
) -> CommandResult<TaskDetailVm> {
    let app = state.app().map_err(command_error)?;
    task_detail_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, DesktopState>,
    input: CreateTaskInputVm,
) -> CommandResult<WorkflowVm> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    let background_app = app.clone_for_background();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        background_app.create_task_from_requirement(CreateTaskInput {
            title: input.title,
            description: input.description,
            requirement_file_name: input.requirement_file_name,
            requirement_content: input.requirement_content,
            workflow: input.workflow,
            workflow_template_id: input.workflow_template_id,
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
    .map_err(command_error)?;
    workflow_vm(&app, &summary.task.id).map_err(command_error)
}

#[tauri::command]
pub fn save_task_workflow(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    input: SaveWorkflowInputVm,
) -> CommandResult<WorkflowVm> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    app.save_task_workflow(&task_id, input.workflow)
        .map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn get_workflow(state: State<'_, DesktopState>, task_id: String) -> CommandResult<WorkflowVm> {
    let app = state.app().map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn get_workflow_templates(
    state: State<'_, DesktopState>,
) -> CommandResult<WorkflowTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.workflow_templates().map_err(command_error)
}

#[tauri::command]
pub fn save_workflow_template(
    state: State<'_, DesktopState>,
    input: SaveWorkflowTemplateInputVm,
) -> CommandResult<WorkflowTemplateStore> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    app.save_workflow_template(input.name, input.workflow)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_workflow_template(
    state: State<'_, DesktopState>,
    template_id: String,
    input: UpdateWorkflowTemplateInputVm,
) -> CommandResult<WorkflowTemplateStore> {
    ensure_workflow_agents_doctor_ready(state.inner(), &input.workflow)?;
    let app = state.app().map_err(command_error)?;
    app.update_workflow_template(&template_id, input.workflow)
        .map_err(command_error)
}

#[tauri::command]
pub fn delete_workflow_template(
    state: State<'_, DesktopState>,
    template_id: String,
) -> CommandResult<WorkflowTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.delete_workflow_template(&template_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_auto_templates(state: State<'_, DesktopState>) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.auto_templates().map_err(command_error)
}

#[tauri::command]
pub fn save_auto_template(
    state: State<'_, DesktopState>,
    input: SaveAutoTemplateInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.save_auto_template(input.name, input.config)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_auto_template(
    state: State<'_, DesktopState>,
    template_id: String,
    input: UpdateAutoTemplateInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.update_auto_template(&template_id, input.name, input.config)
        .map_err(command_error)
}

#[tauri::command]
pub fn delete_auto_template(
    state: State<'_, DesktopState>,
    template_id: String,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.delete_auto_template(&template_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn replace_auto_templates(
    state: State<'_, DesktopState>,
    input: ReplaceAutoTemplatesInputVm,
) -> CommandResult<AutoTemplateStore> {
    let app = state.app().map_err(command_error)?;
    app.replace_auto_templates(input.templates)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_run_detail(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunDetailVm> {
    let app = state.app().map_err(command_error)?;
    run_detail_vm(&app, &task_id, &run_id).map_err(command_error)
}

#[tauri::command]
pub fn get_round_detail(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    selection: Option<RoundSelectionInput>,
) -> CommandResult<RoundDetailVm> {
    let app = state.app().map_err(command_error)?;
    round_detail_vm(&app, &task_id, &run_id, &round_id, selection).map_err(command_error)
}

#[tauri::command]
pub fn start_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
) -> CommandResult<RunSummaryVm> {
    let base_app = state.app().map_err(command_error)?;
    let app = configure_conversation_runtime_callbacks(base_app, app_handle, None);
    app.run_start_background(&task_id, None)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub async fn get_git_capability(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
) -> CommandResult<gold_band::git::GitCapability> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        Ok(gold_band::git::GitRepositoryService::default().probe(&project_root))
    })
    .await
}

#[tauri::command]
pub async fn initialize_git_repository(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
) -> CommandResult<gold_band::git::GitCapability> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        gold_band::git::GitRepositoryService::default()
            .initialize(&project_root)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_source_control_snapshot(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
) -> CommandResult<gold_band::git::GitSourceControlSnapshot> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .snapshot(&project_id, &workspace.workspace_path)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_git_history(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    query: gold_band::git::GitHistoryQuery,
) -> CommandResult<gold_band::git::GitHistoryPage> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .history(&workspace.workspace_path, &query)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_git_commit_detail(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    oid: String,
) -> CommandResult<gold_band::git::GitCommitDetail> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .commit_detail(&workspace.workspace_path, &oid)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_git_commit_review(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    query: gold_band::git::GitCommitReviewQuery,
) -> CommandResult<gold_band::git::GitCommitReview> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .commit_review(&workspace.workspace_path, &query)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_git_commit_reachability(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    query: gold_band::git::GitCommitReachabilityQuery,
) -> CommandResult<gold_band::git::GitCommitReachability> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .commit_reachability(&workspace.workspace_path, &query)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn execute_git_mutation(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    input: gold_band::git::GitMutationRequest,
) -> CommandResult<gold_band::git::GitMutationResult> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        service
            .execute_mutation(&workspace.workspace_path, &input)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_git_comparison(
    state: State<'_, DesktopState>,
    project_id: String,
    source: gold_band::git::GitComparisonSource,
) -> CommandResult<gold_band::git::GitFileComparison> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace_path = source.workspace_path();
        let workspace = service
            .resolve_scoped_workspace(&project_root, workspace_path.map(camino::Utf8Path::new))
            .map_err(command_error)?;
        match &source {
            gold_band::git::GitComparisonSource::GitHubPr {
                host,
                repository,
                pr_number,
                base_oid,
                head_oid,
                path,
                ..
            } => gold_band::git::GitHubCliService::default()
                .pull_request_revision_comparison(
                    &workspace.workspace_path,
                    host,
                    repository,
                    *pr_number,
                    base_oid,
                    head_oid,
                    path,
                )
                .map_err(command_error),
            _ => service
                .comparison(&workspace.workspace_path, &source)
                .map_err(command_error),
        }
    })
    .await
}

#[tauri::command]
pub async fn start_git_operation(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    input: gold_band::git::GitOperationRequest,
) -> CommandResult<gold_band::git::GitOperation> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let workspace = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        let event_app_handle = app_handle.clone();
        let update_sink: gold_band::git::GitOperationUpdateSink = Arc::new(move |operation| {
            let _ = event_app_handle.emit("gold-band://git-operation-updated", operation);
        });
        service
            .start_operation_with_update_sink(&workspace.workspace_path, &input, Some(update_sink))
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn start_git_state_monitor(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    file_runtime: State<'_, crate::workspace_files::WorkspaceFileRuntime>,
    watch_runtime: State<'_, crate::workspace_files::WorkspaceFileWatchRuntime>,
    monitor_runtime: State<'_, crate::git_state_monitor::GitStateMonitorRuntime>,
    project_id: String,
    workspace_path: Option<String>,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    let debounce_ms = app.config.workspace_files.watch_debounce_ms;
    let (identity, targets) = spawn_blocking_command(move || {
        let service = gold_band::git::GitSourceControlService::default();
        let identity = service
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        let targets = service
            .metadata_watch_targets(&identity.workspace_path)
            .map_err(command_error)?;
        Ok((identity, targets))
    })
    .await?;
    watch_runtime.start_workspace(
        app_handle.clone(),
        file_runtime.inner().clone(),
        project_id.clone(),
        identity.workspace_path.as_std_path().to_path_buf(),
        debounce_ms,
    )?;
    if let Err(error) = monitor_runtime.start(
        app_handle,
        project_id.clone(),
        &identity.common_dir,
        &identity.workspace_path,
        targets,
        debounce_ms,
    ) {
        let _ = watch_runtime.stop_workspace(&project_id, identity.workspace_path.as_std_path());
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub async fn stop_git_state_monitor(
    state: State<'_, DesktopState>,
    watch_runtime: State<'_, crate::workspace_files::WorkspaceFileWatchRuntime>,
    monitor_runtime: State<'_, crate::git_state_monitor::GitStateMonitorRuntime>,
    project_id: String,
    workspace_path: Option<String>,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    let identity = spawn_blocking_command(move || {
        gold_band::git::GitSourceControlService::default()
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)
    })
    .await?;
    monitor_runtime.stop(&identity.common_dir, &identity.workspace_path)?;
    watch_runtime.stop_workspace(&project_id, identity.workspace_path.as_std_path())
}

#[tauri::command]
pub async fn get_git_operation(
    operation_id: String,
) -> CommandResult<gold_band::git::GitOperation> {
    spawn_blocking_command(move || {
        gold_band::git::GitSourceControlService::default()
            .get_operation(&operation_id)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn cancel_git_operation(
    operation_id: String,
) -> CommandResult<gold_band::git::GitOperation> {
    spawn_blocking_command(move || {
        gold_band::git::GitSourceControlService::default()
            .cancel_operation(&operation_id)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_github_capability(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
) -> CommandResult<gold_band::git::GitHubCapability> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .capability(&workspace.workspace_path)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn start_github_login(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    host: String,
) -> CommandResult<gold_band::git::GitHubOperation> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        let event_app_handle = app_handle.clone();
        let update_sink: gold_band::git::GitHubOperationUpdateSink = Arc::new(move |operation| {
            let _ = event_app_handle.emit("gold-band://github-operation-updated", operation);
        });
        gold_band::git::GitHubCliService::default()
            .start_login_with_update_sink(&workspace.workspace_path, &host, Some(update_sink))
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_github_operation(
    operation_id: String,
) -> CommandResult<gold_band::git::GitHubOperation> {
    spawn_blocking_command(move || {
        gold_band::git::GitHubCliService::default()
            .get_operation(&operation_id)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn cancel_github_operation(
    operation_id: String,
) -> CommandResult<gold_band::git::GitHubOperation> {
    spawn_blocking_command(move || {
        gold_band::git::GitHubCliService::default()
            .cancel_operation(&operation_id)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn list_github_pull_requests(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    host: String,
    repository: String,
    query: gold_band::git::GitHubPullRequestQuery,
) -> CommandResult<Vec<gold_band::git::GitHubPullRequestSummary>> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .list_pull_requests(&workspace.workspace_path, &host, &repository, &query)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_github_pull_request(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    host: String,
    repository: String,
    number: u64,
) -> CommandResult<gold_band::git::GitHubPullRequestDetail> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .pull_request_detail(&workspace.workspace_path, &host, &repository, number)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn preflight_github_pull_request(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    input: gold_band::git::GitHubPullRequestPreflightInput,
) -> CommandResult<gold_band::git::GitHubPullRequestPreflight> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .preflight_pull_request(&workspace.workspace_path, &input)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn start_github_pull_request_create(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    input: gold_band::git::GitHubPullRequestCreateInput,
) -> CommandResult<gold_band::git::GitHubOperation> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        let event_app_handle = app_handle.clone();
        let update_sink: gold_band::git::GitHubOperationUpdateSink = Arc::new(move |operation| {
            let _ = event_app_handle.emit("gold-band://github-operation-updated", operation);
        });
        gold_band::git::GitHubCliService::default()
            .start_pull_request_create_with_update_sink(
                &workspace.workspace_path,
                input,
                Some(update_sink),
            )
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn list_github_issues(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    host: String,
    repository: String,
    query: gold_band::git::GitHubIssueQuery,
) -> CommandResult<Vec<gold_band::git::GitHubIssueSummary>> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .list_issues(&workspace.workspace_path, &host, &repository, &query)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn get_github_issue(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    host: String,
    repository: String,
    number: u64,
) -> CommandResult<gold_band::git::GitHubIssueDetail> {
    let app = resolve_command_app(state.inner(), Some(&project_id))?;
    let project_root = app.paths.repo_root;
    spawn_blocking_command(move || {
        let git = gold_band::git::GitSourceControlService::default();
        let workspace = git
            .resolve_scoped_workspace(
                &project_root,
                workspace_path.as_deref().map(camino::Utf8Path::new),
            )
            .map_err(command_error)?;
        gold_band::git::GitHubCliService::default()
            .issue_detail(&workspace.workspace_path, &host, &repository, number)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub fn continue_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let app = resolve_command_app_with_emitters(&app_handle, state.inner(), project_id.as_deref())?;
    app.run_continue_background(&task_id, &run_id, None, None)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub async fn continue_conversation_runtime(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let app = resolve_command_app_with_emitters(&app_handle, state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let app = app.clone_for_background();
    spawn_blocking_command(move || {
        let run = app
            .run_status(&locator.task_id, &locator.run_id)
            .map_err(command_error)?;
        let manual_check_pending = current_attempt_manual_check_pending(&app, &locator, &run)?;
        if !runtime_continue_required(&app, &locator, &run, manual_check_pending)? {
            return Err(CommandErrorVm::new(
                "runtime.continue-not-available",
                serde_json::json!({
                    "taskId": locator.task_id,
                    "runId": locator.run_id,
                    "roundId": locator.round_id,
                    "nodeId": locator.node_id,
                    "attemptId": locator.attempt_id,
                }),
            ));
        }
        if client::prompt_activity(&locator.attempt_dir(&app)).is_some() {
            return Err(CommandErrorVm::new(
                "runtime.continue-already-active",
                serde_json::json!({}),
            ));
        }
        let attempt_dir = locator.attempt_dir(&app);
        let model_override = current_acp_session_model_override(&attempt_dir);
        let permission_mode_override = current_acp_session_permission_mode_override(&attempt_dir);
        let run = if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (locator.outer_node_id(), locator.outer_attempt_id())
        {
            app.run_continue_dynamic_inner_background(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                outer_node_id,
                outer_attempt_id,
                &locator.node_id,
                &locator.attempt_id,
                None,
                String::new(),
                Vec::new(),
                model_override,
                permission_mode_override,
            )
        } else {
            app.run_continue_background_with_config_overrides(
                &locator.task_id,
                &locator.run_id,
                None,
                None,
                Vec::new(),
                model_override,
                permission_mode_override,
            )
        }
        .map(run_summary_vm)
        .map_err(command_error)?;
        Ok(ConversationPromptSubmitVm {
            kind: "runtime-continue-started".to_string(),
            session: None,
            run: Some(run),
            lifecycle: runtime_continue_started_lifecycle_for_locator(&app, &locator),
        })
    })
    .await
}

#[tauri::command]
pub fn pause_run(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    project_id: Option<String>,
) -> CommandResult<RunSummaryVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    app.run_pause(&task_id, &run_id, PauseReason::ProcessInterrupted)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub async fn stop_active_session(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ActiveSessionStopVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let direct_mode = conversation_run_mode(&app, &locator.task_id)
        == Some(gold_band::config::ConversationRunMode::Direct);
    if direct_mode {
        request_auto_dispatch_suspension(&locator.attempt_dir(&app)).map_err(command_error)?;
    }
    let operation_id = Uuid::new_v4().to_string();
    let control_app = app.clone_for_background();
    let control_locator = locator.clone();
    let (attempt_dir, current_run, lifecycle) = spawn_blocking_command(move || {
        if direct_mode {
            let queue_attempt_dir = control_locator.attempt_dir(&control_app);
            let queue = load_prompt_queue(&queue_attempt_dir).map_err(command_error)?;
            if !queue.items.is_empty() {
                suspend_auto_dispatch(&queue_attempt_dir).map_err(command_error)?;
            }
        }
        let attempt_dir = persist_active_session_stop(&control_app, &control_locator)?;
        let current_run = control_app
            .run_status(&control_locator.task_id, &control_locator.run_id)
            .map_err(command_error)?;
        let lifecycle = lifecycle_for_locator(&control_app, &control_locator);
        Ok((attempt_dir, current_run, lifecycle))
    })
    .await?;

    spawn_active_session_stop_cleanup(
        app_handle,
        app.clone_for_background(),
        project_id,
        locator.clone(),
        attempt_dir,
    );
    spawn_index_attempt(
        state.inner(),
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    );
    Ok(ActiveSessionStopVm {
        operation_id,
        status: "accepted".to_string(),
        kind: "stop-accepted".to_string(),
        run: Some(run_summary_vm(current_run)),
        session: None,
        lifecycle,
    })
}

#[tauri::command]
pub fn submit_manual_check(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outcome: String,
) -> CommandResult<RunSummaryVm> {
    let app = resolve_command_app_with_emitters(&app_handle, state.inner(), project_id.as_deref())?;
    let outcome = match outcome.as_str() {
        "success" => NodeOutcome::Success,
        "failure" => NodeOutcome::Failure,
        _ => {
            return Err(CommandErrorVm::new(
                "manual-check.invalid-outcome",
                serde_json::json!({ "outcome": outcome }),
            ));
        }
    };
    app.submit_manual_check_background(&task_id, &run_id, &round_id, &node_id, &attempt_id, outcome)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn retry_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let base_app = state.app().map_err(command_error)?;
    let app = configure_conversation_runtime_callbacks(base_app, app_handle, None);
    app.run_retry(&task_id, &run_id)
        .map(run_summary_vm)
        .map_err(command_error)
}

#[tauri::command]
pub fn show_artifact(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
        let artifact_name = name.strip_suffix(".json").unwrap_or(&name);
        let path = app.paths.dynamic_node_artifact_file(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            artifact_name,
        );
        app.artifact_show_path(&path)
    } else {
        app.artifact_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
    };
    content
        .map(|content| ContentVm {
            title: labels.format("detail.artifact", &name),
            kind: "artifact".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn get_log_page(
    state: State<'_, DesktopState>,
    query: LogQueryInput,
) -> CommandResult<LogPageVm> {
    let app = state.app().map_err(command_error)?;
    log_page_vm(&app, query).map_err(command_error)
}

#[tauri::command]
pub fn get_metrics_settings(state: State<'_, DesktopState>) -> CommandResult<MetricsSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let vm = metrics_settings(&context.config);
    eprintln!(
        "[metrics] enabled={} toggle_locked={} base_url={:?} heartbeat={:?} node_metrics={:?} api_key_set={}",
        vm.enabled,
        vm.toggle_locked,
        vm.metrics_base_url,
        vm.heartbeat_endpoint,
        vm.node_metrics_endpoint,
        vm.api_key_set,
    );
    Ok(vm)
}

#[tauri::command]
pub fn update_notification_attention(
    state: State<'_, DesktopState>,
    input: NotificationAttentionInput,
) -> CommandResult<()> {
    state
        .update_notification_attention(input)
        .map_err(command_error)
}

#[tauri::command]
pub fn save_metrics_settings(
    state: State<'_, DesktopState>,
    enabled: bool,
    metrics_base_url: Option<String>,
    api_key: Option<String>,
) -> CommandResult<MetricsSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let mut existing = app.load_settings().map_err(command_error)?;
    existing.desktop_metrics_enabled = Some(enabled);
    existing.desktop_metrics_base_url = metrics_base_url
        .as_deref()
        .and_then(normalize_metrics_base_url);
    existing.desktop_metrics_api_key = api_key.filter(|s| !s.trim().is_empty());
    app.save_settings(&existing).map_err(command_error)?;
    state
        .update_settings_config(&existing)
        .map_err(command_error)?;
    let updated_context = state.context().map_err(command_error)?;
    Ok(metrics_settings(&updated_context.config))
}

pub(crate) fn acp_live_update_emitter(
    app_handle: AppHandle,
    project_id: Option<String>,
    notification_app: Option<App>,
    lifecycle_bus: Option<gold_band::app::observability::RuntimeLifecycleBus>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext, AcpUiEvent) -> anyhow::Result<()> + Send + Sync>
{
    Arc::new(move |context, mut event| {
        let refresh_agent_attention = matches!(
            event.kind.as_str(),
            "permissionRequest" | "elicitationRequest"
        ) && event
            .status
            .as_deref()
            .unwrap_or("pending")
            .eq_ignore_ascii_case("pending");
        maybe_record_agent_commands(&app_handle, notification_app.as_ref(), &context, &event);
        if let Some(lifecycle_bus) = lifecycle_bus.as_ref() {
            let notification_project_id = notification_app
                .as_ref()
                .map(|app| app.paths.project_id.as_str())
                .or(project_id.as_deref());
            if let Some(notification_project_id) = notification_project_id {
                maybe_emit_permission_intervention(
                    lifecycle_bus,
                    notification_project_id,
                    notification_app.as_ref(),
                    &context,
                    &event,
                );
                maybe_emit_elicitation_intervention(
                    lifecycle_bus,
                    notification_project_id,
                    notification_app.as_ref(),
                    &context,
                    &event,
                );
            } else {
                tracing::warn!("project id unavailable; ACP intervention notification dropped");
            }
        }
        compact_live_conversation_event(&mut event);
        emit_acp_event_update(
            &app_handle,
            notification_app.as_ref(),
            project_id.clone(),
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            context.outer_node_id.clone(),
            context.outer_attempt_id.clone(),
            event,
        );
        if refresh_agent_attention && let Some(app) = notification_app.as_ref() {
            let session = if let (Some(outer_node_id), Some(outer_attempt_id)) = (
                context.outer_node_id.as_deref(),
                context.outer_attempt_id.as_deref(),
            ) {
                dynamic_acp_session_vm(
                    app,
                    &context.task_id,
                    &context.run_id,
                    &context.round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &context.node_id,
                    &context.attempt_id,
                    None,
                    None,
                )?
            } else {
                acp_session_vm(
                    app,
                    &context.task_id,
                    &context.run_id,
                    &context.round_id,
                    &context.node_id,
                    &context.attempt_id,
                    None,
                    None,
                )?
            };
            emit_acp_session_update(
                &app_handle,
                app,
                project_id.clone(),
                &context.task_id,
                &context.run_id,
                &context.round_id,
                &context.node_id,
                &context.attempt_id,
                context.outer_node_id.clone(),
                context.outer_attempt_id.clone(),
                session,
            );
        }
        Ok(())
    })
}

fn maybe_record_agent_commands(
    app_handle: &AppHandle,
    app: Option<&App>,
    context: &gold_band::app::AcpLiveEventContext,
    event: &AcpUiEvent,
) {
    if event.kind != "availableCommands" {
        return;
    }
    let Some(commands) = event.raw.as_ref().and_then(parse_available_commands) else {
        return;
    };
    let Some(app) = app else {
        return;
    };
    let locator = AttemptLocator::new(
        context.task_id.clone(),
        context.run_id.clone(),
        context.round_id.clone(),
        context.node_id.clone(),
        context.attempt_id.clone(),
        context.outer_node_id.clone(),
        context.outer_attempt_id.clone(),
    );
    let Some(provider) = acp_turn_provider_id(app, &locator) else {
        return;
    };
    let Ok(agent_id) = ManagedAgentId::from_str(&provider) else {
        return;
    };
    let state = app_handle.state::<DesktopState>();
    if let Ok(catalog) = state.record_agent_commands(&agent_id, &app.paths.repo_root, commands) {
        emit_agent_commands_updated(app_handle, Some(&catalog));
    }
}

/// 路径 B：旁路监听 `permissionRequest` 事件流，强制 `PermissionRequested` 发干预通知。
///
/// 仅当 `event.kind == "permissionRequest" && status == "pending"` 时触发。node_label
/// 优先使用 Direct 会话 Agent identity，其次使用节点实际 provider 展示名，最后才回退 node_id。
/// event_id 包含 request id：同一请求的重复 update 去重，同一 attempt 的后续请求独立通知。
fn maybe_emit_permission_intervention(
    lifecycle_bus: &gold_band::app::observability::RuntimeLifecycleBus,
    project_id: &str,
    app: Option<&App>,
    context: &gold_band::app::AcpLiveEventContext,
    event: &AcpUiEvent,
) {
    if event.kind != "permissionRequest" {
        return;
    }
    let is_pending = event
        .status
        .as_deref()
        .map(|s| s == "pending")
        .unwrap_or(false);
    if !is_pending {
        return;
    }
    lifecycle_bus.emit(RuntimeLifecycleEvent::InterventionRequested {
        event_id: request_scoped_intervention_event_id(
            project_id,
            context,
            event,
            PERMISSION_REQUESTED_DEDUP_SUFFIX,
        ),
        occurred_at: current_timestamp(),
        scheduled_occurrence_id: app
            .and_then(|value| value.scheduled_occurrence_id().map(str::to_string)),
        project_id: project_id.to_string(),
        task_id: context.task_id.clone(),
        run_id: context.run_id.clone(),
        round_id: context.round_id.clone(),
        node_id: context.node_id.clone(),
        attempt_id: context.attempt_id.clone(),
        node_label: acp_intervention_node_label(app, context),
        kind: RuntimeInterventionKind::PermissionRequested,
        task_title: None,
    });
}

fn maybe_emit_elicitation_intervention(
    lifecycle_bus: &gold_band::app::observability::RuntimeLifecycleBus,
    project_id: &str,
    app: Option<&App>,
    context: &gold_band::app::AcpLiveEventContext,
    event: &AcpUiEvent,
) {
    if event.kind != "elicitationRequest" {
        return;
    }
    let is_pending = event
        .status
        .as_deref()
        .map(|s| s == "pending")
        .unwrap_or(false);
    if !is_pending {
        return;
    }
    lifecycle_bus.emit(RuntimeLifecycleEvent::InterventionRequested {
        event_id: request_scoped_intervention_event_id(
            project_id,
            context,
            event,
            ELICITATION_REQUESTED_DEDUP_SUFFIX,
        ),
        occurred_at: current_timestamp(),
        scheduled_occurrence_id: app
            .and_then(|value| value.scheduled_occurrence_id().map(str::to_string)),
        project_id: project_id.to_string(),
        task_id: context.task_id.clone(),
        run_id: context.run_id.clone(),
        round_id: context.round_id.clone(),
        node_id: context.node_id.clone(),
        attempt_id: context.attempt_id.clone(),
        node_label: acp_intervention_node_label(app, context),
        kind: RuntimeInterventionKind::ElicitationRequested,
        task_title: None,
    });
}

fn acp_intervention_node_label(
    app: Option<&App>,
    context: &gold_band::app::AcpLiveEventContext,
) -> String {
    let Some(app) = app else {
        return context.node_id.clone();
    };
    if let Some(agent_label) =
        gold_band::app::direct_conversation_agent_label(app, &context.task_id)
    {
        return agent_label;
    }
    acp_turn_agent_label(
        app,
        &AttemptLocator::new(
            context.task_id.clone(),
            context.run_id.clone(),
            context.round_id.clone(),
            context.node_id.clone(),
            context.attempt_id.clone(),
            context.outer_node_id.clone(),
            context.outer_attempt_id.clone(),
        ),
    )
}

fn request_scoped_intervention_event_id(
    project_id: &str,
    context: &gold_band::app::AcpLiveEventContext,
    event: &AcpUiEvent,
    kind_suffix: &str,
) -> String {
    let request_id = event.id.trim();
    let suffix = if request_id.is_empty() {
        kind_suffix.to_string()
    } else {
        format!("{kind_suffix}:{request_id}")
    };
    gold_band::app::make_dedup_key_with_suffix(
        project_id,
        &context.run_id,
        &context.round_id,
        &context.node_id,
        &context.attempt_id,
        &suffix,
    )
}

pub(crate) fn acp_session_update_emitter(
    app_handle: AppHandle,
    app: gold_band::app::App,
    project_id: Option<String>,
) -> Arc<dyn Fn(gold_band::app::AcpLiveEventContext) -> anyhow::Result<()> + Send + Sync> {
    Arc::new(move |context| {
        let session = if let (Some(outer_node_id), Some(outer_attempt_id)) = (
            context.outer_node_id.as_deref(),
            context.outer_attempt_id.as_deref(),
        ) {
            dynamic_acp_session_vm(
                &app,
                &context.task_id,
                &context.run_id,
                &context.round_id,
                outer_node_id,
                outer_attempt_id,
                &context.node_id,
                &context.attempt_id,
                None,
                None,
            )?
        } else {
            acp_session_vm(
                &app,
                &context.task_id,
                &context.run_id,
                &context.round_id,
                &context.node_id,
                &context.attempt_id,
                None,
                None,
            )?
        };
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id.clone(),
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            context.outer_node_id.clone(),
            context.outer_attempt_id.clone(),
            session,
        );
        Ok(())
    })
}

fn emit_acp_session_update(
    app_handle: &AppHandle,
    app: &App,
    project_id: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
) {
    let activity = conversation_prompt_activity_vm(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    emit_acp_update(
        app_handle,
        Some(app),
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        session,
        None,
        activity,
    );
}

fn emit_acp_event_update(
    app_handle: &AppHandle,
    activity_app: Option<&App>,
    project_id: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    event: AcpUiEvent,
) {
    let activity = activity_app.and_then(|app| {
        conversation_prompt_activity_vm(
            app,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id.as_deref(),
            outer_attempt_id.as_deref(),
        )
    });
    emit_acp_update(
        app_handle,
        None,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        None,
        Some(event),
        activity,
    );
}

#[allow(clippy::too_many_arguments)]
fn conversation_prompt_activity_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> Option<ConversationTaskActivityVm> {
    let attempt_dir = resolve_acp_attempt_dir(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    client::prompt_activity(&attempt_dir).map(conversation_task_activity_from_prompt)
}

#[allow(clippy::too_many_arguments)]
fn emit_acp_update(
    app_handle: &AppHandle,
    app: Option<&App>,
    project_id: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
    activity: Option<ConversationTaskActivityVm>,
) {
    let branch_id = event
        .as_ref()
        .map(gold_band::acp::branches::event_branch_id);
    let lifecycle = app.and_then(|app| {
        conversation_attempt_lifecycle_vm(
            app,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outer_node_id.as_deref(),
            outer_attempt_id.as_deref(),
        )
        .ok()
    });
    let _ = app_handle.emit(
        ACP_SESSION_EVENT,
        AcpSessionUpdatedEventVm {
            branch_id,
            project_id,
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            node_id: node_id.to_string(),
            attempt_id: attempt_id.to_string(),
            outer_node_id,
            outer_attempt_id,
            session,
            event,
            lifecycle,
            activity,
        },
    );
}

fn acp_live_event_context(
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> gold_band::app::AcpLiveEventContext {
    gold_band::app::AcpLiveEventContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id,
        outer_attempt_id,
    }
}

#[tauri::command]
pub fn get_acp_session(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpSessionQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let trace_id = query
        .as_ref()
        .and_then(|query| query.trace_id.as_deref())
        .map(str::trim)
        .filter(|trace_id| !trace_id.is_empty())
        .map(str::to_string);
    let branch_id = query
        .as_ref()
        .and_then(|query| query.branch_id.as_deref())
        .unwrap_or(gold_band::acp::branches::ROOT_BRANCH_ID)
        .to_string();
    let trace_started_at = Instant::now();
    log_acp_session_command_stage(
        trace_id.as_deref(),
        &branch_id,
        "command-received",
        trace_started_at,
    );
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    log_acp_session_command_stage(
        trace_id.as_deref(),
        &branch_id,
        "project-resolved",
        trace_started_at,
    );
    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        client::renew_session_foreground_lease(
            &attempt_dir,
            std::time::Duration::from_secs(app.config.acp_session_foreground_lease_ttl_secs),
        );
        let result = dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            query,
            None,
        )
        .map_err(|error| acp_storage_query_error(error, "acp.session-query-failed"));
        log_acp_session_command_stage(
            trace_id.as_deref(),
            &branch_id,
            "command-complete",
            trace_started_at,
        );
        return result;
    }
    let attempt_dir = app
        .paths
        .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
    client::renew_session_foreground_lease(
        &attempt_dir,
        std::time::Duration::from_secs(app.config.acp_session_foreground_lease_ttl_secs),
    );
    let result = acp_session_vm(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        query,
        None,
    )
    .map_err(|error| acp_storage_query_error(error, "acp.session-query-failed"));
    log_acp_session_command_stage(
        trace_id.as_deref(),
        &branch_id,
        "command-complete",
        trace_started_at,
    );
    result
}

#[tauri::command]
pub fn get_turn_file_change_set(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    branch_id: String,
    change_set_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<TurnFileChangeSet> {
    gold_band::acp::branches::validate_conversation_branch_id(&branch_id).map_err(|_| {
        CommandErrorVm::new("turn-files.version-access-denied", serde_json::json!({}))
    })?;
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let store = TurnFileStore::new(locator.attempt_dir(&app), app.config.turn_files.into());
    let change_set = store
        .load_change_set(&change_set_id)
        .map_err(turn_file_command_error)?;
    if change_set.branch_id != branch_id {
        return Err(CommandErrorVm::new(
            "turn-files.version-access-denied",
            serde_json::json!({}),
        ));
    }
    Ok(change_set)
}

#[tauri::command]
pub fn get_file_comparison(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    branch_id: String,
    change_set_id: String,
    change_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<gold_band::acp::turn_files::FileComparison> {
    gold_band::acp::branches::validate_conversation_branch_id(&branch_id).map_err(|_| {
        CommandErrorVm::new("turn-files.version-access-denied", serde_json::json!({}))
    })?;
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let store = TurnFileStore::new(locator.attempt_dir(&app), app.config.turn_files.into());
    let change_set = store
        .load_change_set(&change_set_id)
        .map_err(turn_file_command_error)?;
    if change_set.branch_id != branch_id {
        return Err(CommandErrorVm::new(
            "turn-files.version-access-denied",
            serde_json::json!({}),
        ));
    }
    store
        .comparison(&change_set_id, &change_id)
        .map_err(turn_file_command_error)
}

fn turn_file_command_error(error: anyhow::Error) -> CommandErrorVm {
    let message = error.to_string();
    let code = if message.starts_with(VERSION_NOT_FOUND) {
        VERSION_NOT_FOUND
    } else if message.starts_with("turn-files.blob-corrupted") {
        "turn-files.blob-corrupted"
    } else {
        CHANGE_SET_NOT_FOUND
    };
    CommandErrorVm::new(code, serde_json::json!({}))
}

fn log_acp_session_command_stage(
    trace_id: Option<&str>,
    branch_id: &str,
    stage: &'static str,
    started_at: Instant,
) {
    let Some(trace_id) = trace_id else {
        return;
    };
    let total_ms = started_at.elapsed().as_millis() as u64;
    info!(
        target: "gold_band_desktop::acp_session_query",
        trace_id,
        branch_id,
        stage,
        total_ms,
        "ACP session command stage"
    );
    #[cfg(debug_assertions)]
    eprintln!(
        "[acp-session-query] trace={trace_id} branch={branch_id} stage={stage} total_ms={total_ms}"
    );
}

#[tauri::command]
pub fn get_acp_activity_detail(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: AcpActivityDetailQueryInput,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<AcpActivityDetailVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    acp_activity_detail_vm_for_attempt(&attempt_dir, query)
        .map_err(|error| acp_storage_query_error(error, "acp.activity-detail-query-failed"))
}

#[tauri::command]
pub fn get_acp_tool_detail(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: AcpToolDetailQueryInput,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<AcpToolDetailVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    acp_tool_detail_vm_for_attempt(&attempt_dir, query)
        .map_err(|error| acp_storage_query_error(error, "acp.tool-detail-query-failed"))
}

#[tauri::command]
pub fn renew_acp_session_lease(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<u64> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    client::renew_session_foreground_lease(
        &attempt_dir,
        std::time::Duration::from_secs(app.config.acp_session_foreground_lease_ttl_secs),
    );
    Ok(app
        .config
        .acp_session_foreground_lease_renew_interval_secs
        .saturating_mul(1000))
}

fn emit_prompt_queue_lifecycle(
    app_handle: &AppHandle,
    app: &App,
    project_id: Option<String>,
    locator: &AttemptLocator,
) -> Option<ConversationAttemptLifecycleVm> {
    emit_acp_session_update(
        app_handle,
        app,
        project_id,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        None,
    );
    lifecycle_for_locator(app, locator)
}

#[tauri::command]
pub fn update_conversation_queued_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    item_id: String,
    content: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ConversationPromptQueueMutationVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    update_queued_prompt(&locator.attempt_dir(&app), &item_id, content)
        .map_err(prompt_queue_command_error)?;
    Ok(ConversationPromptQueueMutationVm {
        lifecycle: emit_prompt_queue_lifecycle(&app_handle, &app, project_id, &locator),
    })
}

#[tauri::command]
pub fn delete_conversation_queued_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    item_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ConversationPromptQueueMutationVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    delete_queued_prompt(&locator.attempt_dir(&app), &item_id)
        .map_err(prompt_queue_command_error)?;
    Ok(ConversationPromptQueueMutationVm {
        lifecycle: emit_prompt_queue_lifecycle(&app_handle, &app, project_id, &locator),
    })
}

#[tauri::command]
pub async fn use_conversation_queued_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    item_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let attempt_dir = locator.attempt_dir(&app);
    if client::prompt_activity(&attempt_dir).is_some()
        || app
            .run_status(&locator.task_id, &locator.run_id)
            .is_ok_and(|run| run.status == RunStatus::Running)
    {
        return Err(CommandErrorVm::new(
            "conversation.prompt-queue-session-busy",
            serde_json::json!({}),
        ));
    }
    let claimed =
        claim_queued_prompt(&attempt_dir, &item_id).map_err(prompt_queue_command_error)?;
    emit_prompt_queue_lifecycle(&app_handle, &app, project_id.clone(), &locator);
    let result = submit_conversation_prompt(
        app_handle.clone(),
        state,
        project_id.clone(),
        locator.task_id.clone(),
        locator.run_id.clone(),
        locator.round_id.clone(),
        locator.node_id.clone(),
        locator.attempt_id.clone(),
        claimed.content.clone(),
        Some(claimed.prompt_id.clone()),
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        (!claimed.attachment_paths.is_empty()).then_some(claimed.attachment_paths.clone()),
    )
    .await;
    let _ = settle_dispatching_prompts(&attempt_dir);
    emit_prompt_queue_lifecycle(&app_handle, &app, project_id.clone(), &locator);
    result
}

#[tauri::command]
pub async fn submit_conversation_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let app = resolve_command_app_with_emitters(&app_handle, state.inner(), project_id.as_deref())?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    crate::view_models_conversation::touch_conversation_activity(&app, &locator.task_id)
        .map_err(command_error)?;
    let run = app
        .run_status(&locator.task_id, &locator.run_id)
        .map_err(command_error)?;
    let direct_mode = conversation_run_mode(&app, &locator.task_id)
        == Some(gold_band::config::ConversationRunMode::Direct);
    let attempt_dir = locator.attempt_dir(&app);
    let live_prompt_active = matches!(
        client::prompt_activity(&attempt_dir),
        Some(
            client::PromptActivity::Starting
                | client::PromptActivity::Accepted
                | client::PromptActivity::Running
        )
    );
    if direct_mode && (live_prompt_active || run.status == RunStatus::Running) {
        enqueue_prompt(&attempt_dir, prompt, attachment_paths.unwrap_or_default())
            .map_err(prompt_queue_command_error)?;
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id,
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            None,
        );
        let lifecycle = lifecycle_for_locator(&app, &locator);
        if client::prompt_activity(&attempt_dir).is_none()
            && app
                .run_status(&locator.task_id, &locator.run_id)
                .is_ok_and(|run| run.status == RunStatus::Completed)
        {
            let _ = app.notify_prompt_turn_finished(
                acp_live_event_context(
                    &locator.task_id,
                    &locator.run_id,
                    &locator.round_id,
                    &locator.node_id,
                    &locator.attempt_id,
                    locator.outer_node_id.clone(),
                    locator.outer_attempt_id.clone(),
                ),
                None,
                true,
            );
        }
        return Ok(ConversationPromptSubmitVm {
            kind: "queued".to_string(),
            session: None,
            run: None,
            lifecycle,
        });
    }
    if direct_mode {
        let queue = load_prompt_queue(&attempt_dir).map_err(command_error)?;
        if queue.items.is_empty() {
            clear_auto_dispatch_suspension(&attempt_dir).map_err(command_error)?;
        } else {
            mark_user_priority(&attempt_dir).map_err(command_error)?;
        }
    }
    ensure_conversation_prompt_available(&app, &locator)?;

    let session = send_acp_prompt(
        app_handle,
        state,
        project_id,
        locator.task_id.clone(),
        locator.run_id.clone(),
        locator.round_id.clone(),
        locator.node_id.clone(),
        locator.attempt_id.clone(),
        prompt,
        prompt_id,
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        attachment_paths,
    )
    .await?;
    Ok(ConversationPromptSubmitVm {
        kind: "acp-session".to_string(),
        session,
        run: None,
        lifecycle: lifecycle_for_locator(&app, &locator),
    })
}

#[tauri::command]
pub async fn send_acp_prompt(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app_with_emitters(&app_handle, state.inner(), project_id.as_deref())?;
    send_acp_prompt_with_app(
        app_handle,
        app,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        prompt,
        prompt_id,
        outer_node_id,
        outer_attempt_id,
        attachment_paths,
    )
    .await
}

pub(crate) async fn send_acp_prompt_with_app(
    app_handle: AppHandle,
    app: App,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    prompt: String,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<Option<AcpSessionVm>> {
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
    let turn_id = prompt_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("turn-{}", uuid::Uuid::new_v4().simple()));
    let prompt_id = Some(turn_id.clone());
    let queued_dispatch = turn_id.starts_with(QUEUED_PROMPT_ID_PREFIX);
    let direct_mode = conversation_run_mode(&app, &locator.task_id)
        == Some(gold_band::config::ConversationRunMode::Direct);
    let agent_label = acp_turn_agent_label(&app, &locator);
    let preflight = ensure_conversation_prompt_available(&app, &locator);
    finish_acp_prompt_preflight(&app, &locator, &turn_id, &agent_label, preflight)?;
    let project_id_for_emit = project_id.clone();
    let project_id_for_spawn = project_id_for_emit.clone();
    let task_id_for_emit = task_id.clone();
    let run_id_for_emit = run_id.clone();
    let round_id_for_emit = round_id.clone();
    let node_id_for_emit = node_id.clone();
    let attempt_id_for_emit = attempt_id.clone();
    let outer_node_id_for_emit = outer_node_id.clone();
    let outer_attempt_id_for_emit = outer_attempt_id.clone();
    let app_for_emit = app.clone_for_background();
    let app_handle_for_task = app_handle.clone();
    let execution = tauri::async_runtime::spawn_blocking(move || -> CommandResult<_> {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            let attempt_dir = app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            );
            if queued_dispatch && auto_dispatch_is_suspended(&attempt_dir) {
                return Err(CommandErrorVm::new(
                    "conversation.prompt-queue-auto-dispatch-suspended",
                    serde_json::json!({}),
                ));
            }
            let worker_ref_path = app.paths.dynamic_node_worker_ref_file(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            );
            let node_path = app.paths.dynamic_node_file(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
            );
            let node = read_json::<gold_band::dynamic::DynamicNodeState>(&node_path)
                .map_err(command_error)?;
            let provider = node.provider.as_deref().ok_or_else(|| {
                CommandErrorVm::new("acp.missing-provider", serde_json::json!({}))
            })?;
            let (_, agent_config) = app.managed_agent(provider).map_err(command_error)?;
            let permission_mode = current_acp_session_permission_mode_override(&attempt_dir)
                .or_else(|| node.permission_mode.clone());
            let model =
                current_acp_session_model_override(&attempt_dir).or_else(|| node.model.clone());
            let (session_mode, continue_ref) = if worker_ref_path.exists() {
                let worker_ref =
                    read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
                (worker_ref.mode, worker_ref.continue_ref)
            } else {
                (SessionMode::New, None)
            };
            let mut prompt_bundle = app
                .dynamic_acp_prompt_bundle_for_attempt(
                    &task_id,
                    &run_id,
                    &round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &node_id,
                    &attempt_id,
                    prompt,
                    prompt_id.clone(),
                    continue_ref.clone(),
                )
                .map_err(command_error)?;
            // Resolve attachments
            if let Some(ref paths) = attachment_paths {
                if !paths.is_empty() {
                    let user_inputs_dir = format!("{}/user-inputs", attempt_dir);
                    let _ = std::fs::create_dir_all(&user_inputs_dir);
                    if let Ok(resolved) =
                        gold_band::provider::resolve_attachments(paths, "user-inputs")
                    {
                        // Copy files to user-inputs/
                        for (r, src) in resolved.iter().zip(paths.iter()) {
                            let src_path = std::path::Path::new(src);
                            if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                                let dest = std::path::Path::new(&user_inputs_dir).join(name);
                                let _ = std::fs::copy(src_path, &dest);
                            }
                            prompt_bundle.attachment_metas.push(r.meta.clone());
                            prompt_bundle.content_blocks.push(r.block.clone());
                        }
                    }
                }
            }
            let app_handle_for_live = app_handle_for_task.clone();
            let task_id_for_live = task_id.clone();
            let run_id_for_live = run_id.clone();
            let round_id_for_live = round_id.clone();
            let node_id_for_live = node_id.clone();
            let attempt_id_for_live = attempt_id.clone();
            let outer_node_id_for_live = Some(outer_node_id.to_string());
            let outer_attempt_id_for_live = Some(outer_attempt_id.to_string());
            let live_update = acp_live_update_emitter(
                app_handle_for_live.clone(),
                project_id_for_spawn.clone(),
                Some(app.clone_for_background()),
                Some(app.lifecycle_bus.clone()),
            );
            let session_update = app.acp_session_update_for(acp_live_event_context(
                &task_id_for_live,
                &run_id_for_live,
                &round_id_for_live,
                &node_id_for_live,
                &attempt_id_for_live,
                outer_node_id_for_live.clone(),
                outer_attempt_id_for_live.clone(),
            ));
            let prompt_accepted = app.acp_prompt_accepted_for(acp_live_event_context(
                &task_id_for_live,
                &run_id_for_live,
                &round_id_for_live,
                &node_id_for_live,
                &attempt_id_for_live,
                outer_node_id_for_live.clone(),
                outer_attempt_id_for_live.clone(),
            ));
            let config_options = current_acp_session_config_option_overrides(&attempt_dir);
            let prompt_run = client::run_prompt(
                provider,
                &agent_config.adapter,
                app.paths.repo_root.clone(),
                app.paths.repo_root.clone(),
                attempt_dir,
                &prompt_bundle,
                session_mode,
                permission_mode,
                model,
                config_options,
                continue_ref,
                app.config.use_local_claude,
                app.config.require_local_claude_executable,
                app.config.acp_session_title_refresh_enabled,
                app.config.acp_raw_max_size_bytes,
                app.config.acp_raw_target_size_bytes,
                client::AcpRuntimePolicy::from(&app.config)
                    .with_external_session_sync_enabled(agent_config.external_session_sync_enabled)
                    .with_system_prompt_support(agent_config.supports_system_prompt()),
                client::AcpOutputPolicy::Conversation,
                Some(&|event| {
                    live_update(
                        acp_live_event_context(
                            &task_id_for_live,
                            &run_id_for_live,
                            &round_id_for_live,
                            &node_id_for_live,
                            &attempt_id_for_live,
                            outer_node_id_for_live.clone(),
                            outer_attempt_id_for_live.clone(),
                        ),
                        event.clone(),
                    )
                }),
                &app.acp_mcp_servers().unwrap_or_else(|e| {
                    eprintln!("WARN: failed to load MCP servers for ACP session: {e}");
                    Vec::new()
                }),
                session_update.as_ref().map(|callback| callback as _),
                prompt_accepted.as_ref().map(|callback| callback as _),
                Some(client::RuntimeStopProbe {
                    run_file: app.paths.run_file(&task_id, &run_id),
                    round_id: round_id.clone(),
                    node_id: node_id.clone(),
                    attempt_id: attempt_id.clone(),
                    attempt_state_file: Some(app.paths.dynamic_node_file(
                        &task_id,
                        &run_id,
                        &round_id,
                        outer_node_id,
                        outer_attempt_id,
                        &node_id,
                    )),
                    turn_control_mode: prompt_bundle.turn_control_mode,
                }),
            )
            .map_err(command_error)?;
            let outcome = acp_turn_outcome(&prompt_run);
            let session = dynamic_acp_session_vm(
                &app,
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
                None,
                None,
            )
            .map_err(command_error)?;
            return Ok((session, outcome));
        }
        let attempt_dir =
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        if queued_dispatch && auto_dispatch_is_suspended(&attempt_dir) {
            return Err(CommandErrorVm::new(
                "conversation.prompt-queue-auto-dispatch-suspended",
                serde_json::json!({}),
            ));
        }
        let worker_ref_path =
            app.paths
                .worker_ref_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let node_path = app
            .paths
            .node_file(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        let node = read_json::<NodeState>(&node_path).map_err(command_error)?;
        let provider = node
            .resolved_config
            .get("provider")
            .and_then(|value| value.as_str())
            .ok_or_else(|| CommandErrorVm::new("acp.missing-provider", serde_json::json!({})))?;
        let (_, agent_config) = app.managed_agent(provider).map_err(command_error)?;
        let permission_mode = current_acp_session_permission_mode_override(&attempt_dir);
        let (session_mode, continue_ref) = if worker_ref_path.exists() {
            let worker_ref =
                read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?;
            (worker_ref.mode, worker_ref.continue_ref)
        } else {
            (SessionMode::New, None)
        };
        let mut prompt_bundle = app
            .acp_prompt_bundle_for_attempt(
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                prompt,
                prompt_id,
                continue_ref.clone(),
            )
            .map_err(command_error)?;
        // Resolve attachments
        if let Some(ref paths) = attachment_paths {
            if !paths.is_empty() {
                let user_inputs_dir = format!("{}/user-inputs", attempt_dir);
                let _ = std::fs::create_dir_all(&user_inputs_dir);
                if let Ok(resolved) = gold_band::provider::resolve_attachments(paths, "user-inputs")
                {
                    for (r, src) in resolved.iter().zip(paths.iter()) {
                        let src_path = std::path::Path::new(src);
                        if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                            let dest = std::path::Path::new(&user_inputs_dir).join(name);
                            let _ = std::fs::copy(src_path, &dest);
                        }
                        prompt_bundle.attachment_metas.push(r.meta.clone());
                        prompt_bundle.content_blocks.push(r.block.clone());
                    }
                }
            }
        }
        let app_handle_for_live = app_handle_for_task.clone();
        let task_id_for_live = task_id.clone();
        let run_id_for_live = run_id.clone();
        let round_id_for_live = round_id.clone();
        let node_id_for_live = node_id.clone();
        let attempt_id_for_live = attempt_id.clone();
        let live_update = acp_live_update_emitter(
            app_handle_for_live.clone(),
            project_id_for_spawn.clone(),
            Some(app.clone_for_background()),
            Some(app.lifecycle_bus.clone()),
        );
        let session_update = app.acp_session_update_for(acp_live_event_context(
            &task_id_for_live,
            &run_id_for_live,
            &round_id_for_live,
            &node_id_for_live,
            &attempt_id_for_live,
            None,
            None,
        ));
        let prompt_accepted = app.acp_prompt_accepted_for(acp_live_event_context(
            &task_id_for_live,
            &run_id_for_live,
            &round_id_for_live,
            &node_id_for_live,
            &attempt_id_for_live,
            None,
            None,
        ));
        let model = current_acp_session_model_override(&attempt_dir);
        let config_options = current_acp_session_config_option_overrides(&attempt_dir);
        let prompt_run = client::run_prompt(
            provider,
            &agent_config.adapter,
            app.paths.repo_root.clone(),
            app.paths.repo_root.clone(),
            attempt_dir,
            &prompt_bundle,
            session_mode,
            permission_mode,
            model,
            config_options,
            continue_ref,
            app.config.use_local_claude,
            app.config.require_local_claude_executable,
            app.config.acp_session_title_refresh_enabled,
            app.config.acp_raw_max_size_bytes,
            app.config.acp_raw_target_size_bytes,
            client::AcpRuntimePolicy::from(&app.config)
                .with_external_session_sync_enabled(agent_config.external_session_sync_enabled)
                .with_system_prompt_support(agent_config.supports_system_prompt()),
            client::AcpOutputPolicy::Conversation,
            Some(&|event| {
                live_update(
                    acp_live_event_context(
                        &task_id_for_live,
                        &run_id_for_live,
                        &round_id_for_live,
                        &node_id_for_live,
                        &attempt_id_for_live,
                        None,
                        None,
                    ),
                    event.clone(),
                )
            }),
            &app.acp_mcp_servers().unwrap_or_else(|e| {
                eprintln!("WARN: failed to load MCP servers for ACP session: {e}");
                Vec::new()
            }),
            session_update.as_ref().map(|callback| callback as _),
            prompt_accepted.as_ref().map(|callback| callback as _),
            Some(client::RuntimeStopProbe {
                run_file: app.paths.run_file(&task_id, &run_id),
                round_id: round_id.clone(),
                node_id: node_id.clone(),
                attempt_id: attempt_id.clone(),
                attempt_state_file: Some(app.paths.node_file(
                    &task_id,
                    &run_id,
                    &round_id,
                    &node_id,
                    &attempt_id,
                )),
                turn_control_mode: prompt_bundle.turn_control_mode,
            }),
        )
        .map_err(command_error)?;
        let outcome = acp_turn_outcome(&prompt_run);
        let session = acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            None,
        )
        .map_err(command_error)?;
        Ok((session, outcome))
    })
    .await;
    let (session, outcome) = match execution {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = clear_auto_dispatch_reply_batch(&locator.attempt_dir(&app_for_emit));
            emit_acp_turn_finished(
                &app_for_emit,
                &locator,
                &turn_id,
                &agent_label,
                AcpTurnOutcome::Failed,
                AcpTurnBatchProgress::terminal(1),
            );
            return Err(error);
        }
        Err(_) => {
            let _ = clear_auto_dispatch_reply_batch(&locator.attempt_dir(&app_for_emit));
            emit_acp_turn_finished(
                &app_for_emit,
                &locator,
                &turn_id,
                &agent_label,
                AcpTurnOutcome::Failed,
                AcpTurnBatchProgress::terminal(1),
            );
            return Err(CommandErrorVm::new(
                "app.task-join-failed",
                serde_json::json!({}),
            ));
        }
    };
    emit_acp_session_update(
        &app_handle,
        &app_for_emit,
        project_id_for_emit,
        &task_id_for_emit,
        &run_id_for_emit,
        &round_id_for_emit,
        &node_id_for_emit,
        &attempt_id_for_emit,
        outer_node_id_for_emit.clone(),
        outer_attempt_id_for_emit.clone(),
        session.clone(),
    );
    if !direct_mode || outcome != AcpTurnOutcome::Completed {
        if outcome != AcpTurnOutcome::Completed {
            let _ = clear_auto_dispatch_reply_batch(&locator.attempt_dir(&app_for_emit));
        }
        emit_acp_turn_finished(
            &app_for_emit,
            &locator,
            &turn_id,
            &agent_label,
            outcome,
            AcpTurnBatchProgress::terminal(1),
        );
    }
    let _ = app_for_emit.notify_prompt_turn_finished(
        acp_live_event_context(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
        ),
        Some(turn_id.clone()),
        outcome == AcpTurnOutcome::Completed,
    );

    // Fire-and-forget: index this attempt for cross-session search
    spawn_index_attempt_for_app(
        &app_for_emit,
        &task_id_for_emit,
        &run_id_for_emit,
        &round_id_for_emit,
        &node_id_for_emit,
        &attempt_id_for_emit,
        outer_node_id_for_emit.as_deref(),
        outer_attempt_id_for_emit.as_deref(),
    );

    Ok(session)
}

#[tauri::command]
pub fn respond_acp_permission(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    request_id: String,
    option_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let session = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        write_acp_permission_response_signal(&attempt_dir, &request_id, option_id.clone())
            .map_err(command_error)?;
        dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
            None,
            None,
        )
        .map_err(command_error)?
    } else {
        let attempt_dir =
            app.paths
                .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id);
        write_acp_permission_response_signal(&attempt_dir, &request_id, option_id.clone())
            .map_err(command_error)?;
        acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &app,
        project_id.clone(),
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.clone(),
        outer_attempt_id.clone(),
        session.clone(),
    );
    spawn_index_attempt(
        state.inner(),
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    Ok(session)
}

fn stop_acp_session(
    app_handle: AppHandle,
    state: &DesktopState,
    project_id: Option<String>,
    locator: AttemptLocator,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state, project_id.as_deref())?;
    let runtime_was_controlled = attempt_is_runtime_controlled(&app, &locator)?;
    let requested_at = current_timestamp();
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    );
    cancel_pending_permission_requests(&attempt_dir, requested_at.clone())
        .map_err(command_error)?;
    cancel_pending_elicitation_requests(&attempt_dir, requested_at).map_err(command_error)?;

    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        app.pause_dynamic_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
            PauseReason::ProcessInterrupted,
        )
        .map_err(command_error)?;
    } else {
        app.pause_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            PauseReason::ProcessInterrupted,
        )
        .map_err(command_error)?;
    }

    request_acp_cancel_and_persist_interrupted_snapshot(&app, &attempt_dir);
    if runtime_was_controlled {
        gold_band::acp::control::mark_runtime_interrupted(&attempt_dir).map_err(command_error)?;
    }

    let session = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        dynamic_acp_session_vm(
            &app,
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
            &locator.attempt_id,
            None,
            None,
        )
        .map_err(command_error)?
    } else {
        acp_session_vm(
            &app,
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            None,
            None,
        )
        .map_err(command_error)?
    };
    emit_acp_session_update(
        &app_handle,
        &app,
        project_id,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        session.clone(),
    );
    spawn_index_attempt(
        state,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    );
    Ok(session)
}

fn persist_active_session_stop(
    app: &gold_band::app::App,
    locator: &AttemptLocator,
) -> CommandResult<Utf8PathBuf> {
    let attempt_dir = locator.attempt_dir(app);
    let runtime_was_controlled = attempt_is_runtime_controlled(app, locator)?;
    if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        app.pause_dynamic_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
            PauseReason::ProcessInterrupted,
        )
        .map_err(command_error)?;
    } else {
        app.pause_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            PauseReason::ProcessInterrupted,
        )
        .map_err(command_error)?;
    }
    app.persist_cancelled_session_snapshot(&attempt_dir)
        .map_err(command_error)?;
    if runtime_was_controlled {
        gold_band::acp::control::mark_runtime_interrupted(&attempt_dir).map_err(command_error)?;
    }
    client::request_prompt_cancel(&attempt_dir);
    Ok(attempt_dir)
}

fn spawn_active_session_stop_cleanup(
    app_handle: AppHandle,
    app: gold_band::app::App,
    project_id: Option<String>,
    locator: AttemptLocator,
    attempt_dir: Utf8PathBuf,
) {
    tauri::async_runtime::spawn_blocking(move || {
        app.cancel_attempt_dir_best_effort(&attempt_dir);
        if let Err(error) = client::cancel_attempt_prompt(&attempt_dir) {
            warn!(%error, %attempt_dir, "failed to dispatch accepted ACP stop request");
        }
        emit_acp_session_update(
            &app_handle,
            &app,
            project_id,
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            None,
        );
    });
}

fn request_acp_cancel_and_persist_interrupted_snapshot(
    app: &gold_band::app::App,
    attempt_dir: &camino::Utf8Path,
) {
    app.cancel_attempt_dir_best_effort(attempt_dir);
    app.request_attempt_prompt_cancel_best_effort(attempt_dir);
    app.persist_cancelled_session_snapshot_best_effort(attempt_dir);
}

fn spawn_index_attempt(
    state: &DesktopState,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) {
    let Ok(app) = state.app() else { return };
    spawn_index_attempt_for_app(
        &app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
}

fn spawn_index_attempt_for_app(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) {
    let attempt_dir = resolve_acp_attempt_dir(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let ctx = AttemptIndexContext {
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id: outer_node_id.map(String::from),
        outer_attempt_id: outer_attempt_id.map(String::from),
    };
    tauri::async_runtime::spawn_blocking(move || {
        sqlite::index_attempt_with_retry(&attempt_dir, &ctx);
    });
}

fn canonical_permission_request_id(attempt_dir: &camino::Utf8Path, request_id: &str) -> String {
    let stripped_request_id = strip_permission_display_prefix(request_id);
    let candidates = [request_id.to_string(), stripped_request_id.clone()];
    for candidate in candidates {
        let path = gold_band::acp::permission::pending_permission_file(attempt_dir, &candidate);
        if let Ok(pending) = read_json::<PendingPermissionState>(&path) {
            return pending.request_id;
        }
    }
    stripped_request_id
}

fn strip_permission_display_prefix(request_id: &str) -> String {
    let mut current = request_id;
    while let Some(next) = current.strip_prefix("permission-") {
        current = next;
    }
    current.to_string()
}

fn write_acp_permission_response_signal(
    attempt_dir: &camino::Utf8Path,
    request_id: &str,
    option_id: Option<String>,
) -> anyhow::Result<bool> {
    let canonical_request_id = canonical_permission_request_id(attempt_dir, request_id);
    write_permission_response_if_pending(
        attempt_dir,
        &canonical_request_id,
        option_id,
        false,
        current_timestamp(),
    )
}

#[tauri::command]
pub fn cancel_acp_session(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    stop_acp_session(app_handle, state.inner(), project_id, locator)
}

#[tauri::command]
pub async fn get_acp_raw_frames(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    query: Option<AcpRawFrameQueryInput>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<AcpRawFramePageVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        if let (Some(outer_node_id), Some(outer_attempt_id)) =
            (outer_node_id.as_deref(), outer_attempt_id.as_deref())
        {
            let path = app
                .paths
                .dynamic_node_attempt_dir(
                    &task_id,
                    &run_id,
                    &round_id,
                    outer_node_id,
                    outer_attempt_id,
                    &node_id,
                    &attempt_id,
                )
                .join("acp.raw.jsonl");
            return super::view_models::acp_raw_frame_page_vm_for_path(
                &path,
                query.unwrap_or(AcpRawFrameQueryInput {
                    page: None,
                    page_size: None,
                    search: None,
                    kind: None,
                    direction: None,
                    order: None,
                }),
            )
            .map_err(command_error);
        }
        acp_raw_frame_page_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            query.unwrap_or(AcpRawFrameQueryInput {
                page: None,
                page_size: None,
                search: None,
                kind: None,
                direction: None,
                order: None,
            }),
        )
        .map_err(command_error)
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub fn show_attachment(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    name: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
        let path = app
            .paths
            .dynamic_node_attachments_dir(
                &task_id,
                &run_id,
                &round_id,
                outer_node_id,
                outer_attempt_id,
                &node_id,
                &attempt_id,
            )
            .join(&name);
        app.artifact_show_path(&path)
    } else {
        app.attachment_show(&task_id, &run_id, &round_id, &node_id, &attempt_id, &name)
    };
    content
        .map(|content| ContentVm {
            title: labels.format("detail.attachment", &name),
            kind: "attachment".to_string(),
            content,
            metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn show_worker_ref(
    state: State<'_, DesktopState>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<ContentVm> {
    let app = state.app().map_err(command_error)?;
    let labels = Translator::new(app.config.desktop_language);
    let content = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&outer_node_id, &outer_attempt_id)
    {
        let path = app.paths.dynamic_node_worker_ref_file(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        );
        if path.exists() {
            Some(
                std::fs::read_to_string(path.as_std_path())
                    .map_err(|error| command_error(error.into()))?,
            )
        } else {
            None
        }
    } else {
        app.worker_ref_show(&task_id, &run_id, &round_id, &node_id, &attempt_id)
            .map_err(command_error)?
    };
    Ok(ContentVm {
        title: labels.format("detail.workerRef", &node_id),
        kind: "worker-ref".to_string(),
        content: content.unwrap_or_else(|| labels.tr("fallback.missingWorkerRef")),
        metadata: serde_json::json!({ "nodeId": node_id, "attemptId": attempt_id, "outerNodeId": outer_node_id, "outerAttemptId": outer_attempt_id }),
    })
}

#[tauri::command]
pub fn save_desktop_preferences(
    state: State<'_, DesktopState>,
    theme: DesktopThemePreference,
    language: DesktopLanguage,
    font: DesktopFontPreference,
    use_local_claude: bool,
    verbose_logging: bool,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    if context.config.use_local_claude != use_local_claude {
        ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
        gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
            .map_err(command_error)?;
    }
    app.set_user_desktop_preferences(theme, language, font.clone())
        .map_err(command_error)?;
    app.set_user_use_local_claude(use_local_claude)
        .map_err(command_error)?;
    let settings = app
        .set_user_verbose_logging(verbose_logging)
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let log_level = settings.log_level.unwrap_or(context.config.log_level);
    set_runtime_log_level(log_level);
    Ok(preferences_vm(
        theme,
        language,
        font,
        use_local_claude,
        log_level,
        load_avatar_preferences(&app.paths.user_gold_band_dir()).map_err(avatar_command_error)?,
    ))
}

#[tauri::command]
pub fn save_desktop_avatar(
    state: State<'_, DesktopState>,
    input: SaveDesktopAvatarInput,
) -> CommandResult<AvatarPreferencesVm> {
    let app = state.context().map_err(command_error)?.app();
    save_avatar_image(&app.paths.user_gold_band_dir(), input).map_err(avatar_command_error)
}

#[tauri::command]
pub fn select_recent_desktop_avatar(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
    avatar_id: String,
) -> CommandResult<AvatarPreferencesVm> {
    let app = state.context().map_err(command_error)?.app();
    select_recent_avatar(&app.paths.user_gold_band_dir(), kind, &avatar_id)
        .map_err(avatar_command_error)
}

#[tauri::command]
pub fn save_desktop_avatar_shape(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
    shape: AvatarShape,
) -> CommandResult<AvatarPreferencesVm> {
    let app = state.context().map_err(command_error)?.app();
    save_avatar_shape(&app.paths.user_gold_band_dir(), kind, shape).map_err(avatar_command_error)
}

#[tauri::command]
pub fn clear_desktop_avatar(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
) -> CommandResult<AvatarPreferencesVm> {
    let app = state.context().map_err(command_error)?.app();
    clear_avatar(&app.paths.user_gold_band_dir(), kind).map_err(avatar_command_error)
}

fn avatar_command_error(error: crate::avatar::AvatarError) -> CommandErrorVm {
    CommandErrorVm::new(error.code, error.params)
}

#[tauri::command]
pub fn save_updater_settings(
    state: State<'_, DesktopState>,
    override_url: Option<String>,
) -> CommandResult<UpdaterSettingsVm> {
    let override_url = normalize_updater_url_override(override_url).map_err(command_error)?;
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let settings = app
        .set_user_desktop_updater_url_override(override_url)
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let config = state.context().map_err(command_error)?.config;
    let settings = updater_settings(&config);
    Ok(settings)
}

#[tauri::command]
pub fn get_update_status(state: State<'_, DesktopState>) -> CommandResult<UpdateStatusVm> {
    state.update_status().map_err(command_error)
}

#[tauri::command]
pub fn mark_settings_update_seen(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::SettingsEntry, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub fn mark_settings_advanced_update_seen(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::SettingsAdvanced, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub fn dismiss_update_announcement(
    state: State<'_, DesktopState>,
    version: String,
) -> CommandResult<UpdateBadgeStateVm> {
    let config = state
        .mark_update_badge_seen(UpdateBadgeSeenTarget::Announcement, version)
        .map_err(command_error)?;
    Ok(UpdateBadgeStateVm {
        settings_entry_seen_version: config.desktop_update_badges.settings_entry_seen_version,
        settings_advanced_seen_version: config.desktop_update_badges.settings_advanced_seen_version,
        announcement_closed_version: config.desktop_update_badges.announcement_closed_version,
    })
}

#[tauri::command]
pub async fn check_update_manual(app: AppHandle) -> CommandResult<UpdateStatusVm> {
    Ok(check_update(&app, false).await)
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> CommandResult<()> {
    run_download_and_install_update(&app)
        .await
        .map_err(command_error)?;
    crate::desktop_lifecycle::request_app_restart(&app)
}

fn providers_for_node(node: &NodeDsl) -> Vec<String> {
    match node {
        NodeDsl::Worker(worker) => worker.provider.iter().cloned().collect(),
        NodeDsl::AiDynamic(dynamic) => match &dynamic.agent_strategy {
            AiDynamicAgentStrategy::Fixed { provider, .. } => vec![provider.clone()],
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider,
                available_agents,
                ..
            } => {
                let mut providers = vec![bootstrap_provider.clone()];
                for agent_ref in available_agents {
                    if !providers.contains(&agent_ref.provider) {
                        providers.push(agent_ref.provider.clone());
                    }
                }
                providers
            }
        },
    }
}

fn ensure_workflow_agents_doctor_ready(
    state: &DesktopState,
    workflow: &WorkflowDsl,
) -> CommandResult<()> {
    let diagnostics = state.agent_diagnostics().map_err(command_error)?;
    for node in &workflow.nodes {
        for provider in providers_for_node(node) {
            let agent_id = ManagedAgentId::from_str(&provider).map_err(command_error)?;
            match diagnostics.get(&agent_id) {
                Some(diagnostic) if diagnostic.available => {}
                Some(diagnostic) => {
                    return Err(CommandErrorVm::new(
                        "workflow.agent-doctor-failed",
                        serde_json::json!({ "agentType": provider, "reason": diagnostic.reason }),
                    ));
                }
                None => {
                    return Err(CommandErrorVm::new(
                        "workflow.agent-doctor-required",
                        serde_json::json!({ "agentType": provider }),
                    ));
                }
            }
        }
    }
    let app = state.app().map_err(command_error)?;
    let validated = gold_band::dsl::validate_workflow(workflow.clone()).map_err(command_error)?;
    app.validate_workflow_agents(&validated)
        .map_err(command_error)
}

pub fn command_error(error: anyhow::Error) -> CommandErrorVm {
    if let Some(error) = error.downcast_ref::<gold_band::git::GitPreflightError>() {
        return CommandErrorVm::new(error.code, error.params());
    }
    if let Some(error) = error.downcast_ref::<gold_band::git::GitServiceError>() {
        return CommandErrorVm::new(error.code, error.params.clone());
    }
    if let Some(error) = error.downcast_ref::<gold_band::git::GitHubServiceError>() {
        return CommandErrorVm::new(error.code, error.params.clone());
    }
    if let Some(error) = error.downcast_ref::<gold_band::runtime_error::RuntimeError>() {
        return CommandErrorVm::new(error.info.code_str(), error.info.params.clone());
    }
    if let Some(error) = error.downcast_ref::<WorkflowValidationError>() {
        return workflow_validation_command_error(error);
    }
    if let Some(error) = error.downcast_ref::<SkillCommandError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    if let Some(error) = error.downcast_ref::<ProfileCommandError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    if let Some(error) = error.downcast_ref::<gold_band::acp::branches::ConversationBranchError>() {
        return CommandErrorVm::new(error.code(), serde_json::json!({}));
    }
    let message = error.to_string();
    if let Some(code) = updater_command_error_code(&message) {
        return CommandErrorVm::new(code, serde_json::json!({ "message": message }));
    }
    CommandErrorVm::new("app.unexpected", serde_json::json!({ "message": message }))
}

fn prompt_queue_command_error(error: PromptQueueError) -> CommandErrorVm {
    let code = match error {
        PromptQueueError::Full => "conversation.prompt-queue-full",
        PromptQueueError::NotFound => "conversation.prompt-queue-item-not-found",
        PromptQueueError::Dispatching => "conversation.prompt-queue-item-dispatching",
        PromptQueueError::Empty => "conversation.prompt-queue-empty",
        PromptQueueError::Storage => "conversation.prompt-queue-storage-failed",
    };
    CommandErrorVm::new(code, serde_json::json!({}))
}

fn acp_storage_query_error(error: anyhow::Error, fallback_code: &'static str) -> CommandErrorVm {
    if error
        .downcast_ref::<gold_band::acp::branches::ConversationBranchError>()
        .is_some()
    {
        return command_error(error);
    }
    CommandErrorVm::new(fallback_code, serde_json::json!({}))
}

fn updater_command_error_code(message: &str) -> Option<&'static str> {
    if message.contains("updater.invalid-url") {
        Some("updater.invalid-url")
    } else if message.contains("updater.no-update") {
        Some("updater.no-update")
    } else if message.contains("updater.install-failed") {
        Some("updater.install-failed")
    } else if message.contains("updater.check-failed") {
        Some("updater.check-failed")
    } else {
        None
    }
}

fn workflow_validation_command_error(error: &WorkflowValidationError) -> CommandErrorVm {
    match error {
        WorkflowValidationError::MissingEndNode => {
            CommandErrorVm::new("workflow.missing-end-node", serde_json::json!({}))
        }
        WorkflowValidationError::UnreachableNode { node_id } => CommandErrorVm::new(
            "workflow.unreachable-node",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::SuccessNewRoundTarget { from } => CommandErrorVm::new(
            "workflow.success-new-round-target",
            serde_json::json!({ "from": from }),
        ),
        WorkflowValidationError::MissingNewRoundEntry { from } => CommandErrorVm::new(
            "workflow.missing-new-round-entry",
            serde_json::json!({ "from": from }),
        ),
        WorkflowValidationError::InvalidNewRoundEntry { from, entry } => CommandErrorVm::new(
            "workflow.invalid-new-round-entry",
            serde_json::json!({ "from": from, "entry": entry }),
        ),
        WorkflowValidationError::DuplicateWorkflowId {
            workflow_name,
            workflow_id,
            conflicts,
        } => CommandErrorVm::new(
            "workflow.duplicate-id",
            serde_json::json!({
                "workflowName": workflow_name,
                "workflowId": workflow_id,
                "conflicts": conflicts,
            }),
        ),
        WorkflowValidationError::AiDynamicInvalidWorkflow {
            node_id,
            workflow_name,
            reason,
        } => CommandErrorVm::new(
            "workflow.ai-dynamic-invalid-workflow",
            serde_json::json!({
                "nodeId": node_id,
                "workflowName": workflow_name,
                "reason": reason,
            }),
        ),
        WorkflowValidationError::WorkerModelBlank { node_id, provider } => CommandErrorVm::new(
            "workflow.model-blank",
            serde_json::json!({ "nodeId": node_id, "provider": provider }),
        ),
        WorkflowValidationError::DynamicFixedModelBlank { node_id } => CommandErrorVm::new(
            "workflow.dynamic-fixed-model-blank",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::DynamicAgentsEmpty { node_id } => CommandErrorVm::new(
            "workflow.dynamic-agents-empty",
            serde_json::json!({ "nodeId": node_id }),
        ),
        WorkflowValidationError::DynamicAgentDuplicate { node_id, provider } => {
            CommandErrorVm::new(
                "workflow.dynamic-agent-duplicate",
                serde_json::json!({ "nodeId": node_id, "provider": provider }),
            )
        }
        WorkflowValidationError::DynamicAgentModelBlank { node_id, provider } => {
            CommandErrorVm::new(
                "workflow.dynamic-agent-model-blank",
                serde_json::json!({ "nodeId": node_id, "provider": provider }),
            )
        }
        WorkflowValidationError::AgentModelBlank { provider } => CommandErrorVm::new(
            "workflow.agent-model-blank",
            serde_json::json!({ "provider": provider }),
        ),
    }
}

// ── SQLite search commands ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchAcpPromptsInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchAcpSessionsInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

fn set_acp_config_option_current_value(
    value: &mut serde_json::Value,
    category_or_id: &str,
    next_value: &str,
) {
    let Some(options) = value
        .get_mut("configOptions")
        .and_then(|options| options.as_array_mut())
    else {
        return;
    };
    if let Some(option) = options.iter_mut().find(|option| {
        option.get("id").and_then(|item| item.as_str()) == Some(category_or_id)
            || option.get("category").and_then(|item| item.as_str()) == Some(category_or_id)
    }) {
        if let Some(object) = option.as_object_mut() {
            object.insert(
                "currentValue".to_string(),
                serde_json::Value::String(next_value.to_string()),
            );
        }
    }
}

fn current_acp_session_override(attempt_dir: &Utf8PathBuf, override_key: &str) -> Option<String> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return None;
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| {
            value
                .get(override_key)
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        })
}

fn current_acp_session_model_override(attempt_dir: &Utf8PathBuf) -> Option<String> {
    current_acp_session_override(attempt_dir, "modelOverride")
}

fn current_acp_session_permission_mode_override(attempt_dir: &Utf8PathBuf) -> Option<String> {
    current_acp_session_override(attempt_dir, "permissionModeOverride")
}

fn current_acp_session_config_option_overrides(
    attempt_dir: &Utf8PathBuf,
) -> std::collections::BTreeMap<String, String> {
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Default::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value.get("configOptionOverrides").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchTasksInput {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

#[tauri::command]
pub async fn search_tasks(
    state: State<'_, DesktopState>,
    input: SearchTasksInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::TaskSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
        index.search_tasks(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub async fn set_acp_session_model(
    _app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    model_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Ok(None);
    };

    let session_json = std::fs::read_to_string(&path).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value: serde_json::Value = serde_json::from_str(&session_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    if let Some(session) = value.as_object_mut() {
        if let Some(model_id) = model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            session.insert(
                "modelOverride".to_string(),
                serde_json::Value::String(model_id.to_string()),
            );
        } else {
            session.remove("modelOverride");
        }
    }
    if let Some(model_id) = model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(models) = value.get_mut("models").and_then(|m| m.as_object_mut()) {
            models.insert(
                "currentModelId".to_string(),
                serde_json::Value::String(model_id.to_string()),
            );
        }
        set_acp_config_option_current_value(&mut value, "model", model_id);
    }

    let updated_json = serde_json::to_string_pretty(&value).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-serialize-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    std::fs::write(&path, &updated_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-write-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    let vm = if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        crate::view_models::dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            on,
            oa,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    } else {
        crate::view_models::acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    };
    Ok(vm.map_err(command_error)?)
}

#[tauri::command]
pub async fn set_acp_session_permission_mode(
    _app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    permission_mode_id: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Ok(None);
    };

    let session_json = std::fs::read_to_string(&path).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value: serde_json::Value = serde_json::from_str(&session_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    if let Some(session) = value.as_object_mut() {
        if let Some(permission_mode_id) = permission_mode_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            session.insert(
                "permissionModeOverride".to_string(),
                serde_json::Value::String(permission_mode_id.to_string()),
            );
        } else {
            session.remove("permissionModeOverride");
        }
    }
    if let Some(permission_mode_id) = permission_mode_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(modes) = value.get_mut("modes").and_then(|m| m.as_object_mut()) {
            modes.insert(
                "currentModeId".to_string(),
                serde_json::Value::String(permission_mode_id.to_string()),
            );
        }
        set_acp_config_option_current_value(&mut value, "mode", permission_mode_id);
    }

    let updated_json = serde_json::to_string_pretty(&value).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-serialize-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    std::fs::write(&path, &updated_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-write-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    let vm = if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        crate::view_models::dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            on,
            oa,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    } else {
        crate::view_models::acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    };
    Ok(vm.map_err(command_error)?)
}

#[tauri::command]
pub async fn set_acp_session_config_option(
    _app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    option_id: String,
    option_value: Option<String>,
) -> CommandResult<Option<AcpSessionVm>> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    let attempt_dir = resolve_acp_attempt_dir(
        &app,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id.as_deref(),
        outer_attempt_id.as_deref(),
    );
    let snapshot_path = attempt_dir.join("acp.snapshot.json");
    let session_path = attempt_dir.join("acp.session.json");
    let path = if snapshot_path.exists() {
        snapshot_path
    } else if session_path.exists() {
        session_path
    } else {
        return Ok(None);
    };
    let session_json = std::fs::read_to_string(&path).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value: serde_json::Value = serde_json::from_str(&session_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let option_id = option_id.trim();
    let config_option = value
        .get("configOptions")
        .and_then(serde_json::Value::as_array)
        .and_then(|options| {
            options.iter().find(|option| {
                option.get("id").and_then(serde_json::Value::as_str) == Some(option_id)
            })
        })
        .cloned()
        .ok_or_else(|| {
            CommandErrorVm::new(
                "acp.config-option-not-found",
                serde_json::json!({ "optionId": option_id }),
            )
        })?;
    if config_option
        .get("type")
        .and_then(serde_json::Value::as_str)
        != Some("select")
    {
        return Err(CommandErrorVm::new(
            "acp.config-option-not-select",
            serde_json::json!({ "optionId": option_id }),
        ));
    }
    let normalized_value = option_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(selected) = normalized_value {
        let supported = config_option
            .get("options")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|options| {
                options.iter().any(|option| {
                    option.get("value").and_then(serde_json::Value::as_str) == Some(selected)
                })
            });
        if !supported {
            return Err(CommandErrorVm::new(
                "acp.config-option-value-unsupported",
                serde_json::json!({ "optionId": option_id, "value": selected }),
            ));
        }
    }
    if let Some(session) = value.as_object_mut() {
        let overrides = session
            .entry("configOptionOverrides")
            .or_insert_with(|| serde_json::json!({}));
        if !overrides.is_object() {
            *overrides = serde_json::json!({});
        }
        if let Some(overrides) = overrides.as_object_mut() {
            if let Some(selected) = normalized_value {
                overrides.insert(
                    option_id.to_string(),
                    serde_json::Value::String(selected.to_string()),
                );
            } else {
                overrides.remove(option_id);
            }
            if overrides.is_empty() {
                session.remove("configOptionOverrides");
            }
        }
    }
    if let Some(selected) = normalized_value {
        set_acp_config_option_current_value(&mut value, option_id, selected);
    }
    let updated_json = serde_json::to_string_pretty(&value).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-serialize-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    std::fs::write(&path, &updated_json).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-write-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let vm = if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
        crate::view_models::dynamic_acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            on,
            oa,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    } else {
        crate::view_models::acp_session_vm(
            &app,
            &task_id,
            &run_id,
            &round_id,
            &node_id,
            &attempt_id,
            None,
            Some(value),
        )
    };
    Ok(vm.map_err(command_error)?)
}

#[tauri::command]
pub async fn search_acp_prompts(
    state: State<'_, DesktopState>,
    input: SearchAcpPromptsInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::PromptSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
        index.search_prompts(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub async fn search_acp_sessions(
    state: State<'_, DesktopState>,
    input: SearchAcpSessionsInput,
) -> CommandResult<Vec<gold_band::storage::sqlite::SessionSearchResult>> {
    let _ = state.app().map_err(command_error)?;
    let limit = input.limit.min(200);
    let query = input.query;
    tauri::async_runtime::spawn_blocking(move || {
        let index = gold_band::storage::sqlite::search_index().ok_or_else(|| {
            CommandErrorVm::new("search.index-unavailable", serde_json::json!({}))
        })?;
        index.search_sessions(&query, limit).map_err(|e| {
            CommandErrorVm::new(
                "search.query-failed",
                serde_json::json!({ "message": e.to_string() }),
            )
        })
    })
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?
}

#[tauri::command]
pub fn open_in_file_manager(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    // outer_node_id is the container node (e.g. "ai-dynamic"),
    // node_id is the actual dynamic internal node (e.g. "create-hello-world-python-class").
    let path = match (&outer_node_id, &outer_attempt_id, &node_id, &attempt_id) {
        (Some(onid), Some(oaid), nid, aid) => {
            let p = app.paths.dynamic_node_attempt_dir(
                &task_id,
                &run_id,
                &round_id,
                onid,
                oaid,
                nid,
                aid.as_deref().unwrap_or(""),
            );
            eprintln!("[open_in_file_manager] dynamic path: {}", p);
            p
        }
        _ => {
            let p = if let Some(aid) = &attempt_id {
                app.paths
                    .attempt_dir(&task_id, &run_id, &round_id, &node_id, aid)
            } else {
                app.paths.node_dir(&task_id, &run_id, &round_id, &node_id)
            };
            eprintln!("[open_in_file_manager] path: {}", p);
            p
        }
    };
    open_path(path.as_std_path()).map_err(|e| {
        CommandErrorVm::new(
            "file-manager.open-failed",
            serde_json::json!({ "message": e }),
        )
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectoryInput {
    pub project_id: Option<String>,
    pub task_id: String,
    pub run_id: String,
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    #[serde(default)]
    pub relative_path: String,
}

fn conversation_directory_path(
    input: &ConversationDirectoryInput,
    state: &DesktopState,
) -> CommandResult<PathBuf> {
    let app = resolve_command_app(state, input.project_id.as_deref())?;
    let root = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (&input.outer_node_id, &input.outer_attempt_id)
    {
        app.paths.dynamic_node_attempt_dir(
            &input.task_id,
            &input.run_id,
            &input.round_id,
            outer_node_id,
            outer_attempt_id,
            &input.node_id,
            &input.attempt_id,
        )
    } else {
        app.paths.attempt_dir(
            &input.task_id,
            &input.run_id,
            &input.round_id,
            &input.node_id,
            &input.attempt_id,
        )
    };
    let root = std::fs::canonicalize(root.as_std_path()).map_err(|error| {
        CommandErrorVm::new(
            "conversation-directory.not-found",
            serde_json::json!({ "reason": error.to_string() }),
        )
    })?;
    let relative = Path::new(&input.relative_path);
    if input.relative_path.contains('\0')
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(CommandErrorVm::new(
            "conversation-directory.path-outside-root",
            serde_json::json!({ "path": input.relative_path }),
        ));
    }
    let path = std::fs::canonicalize(root.join(relative)).map_err(|error| {
        CommandErrorVm::new(
            "conversation-directory.not-found",
            serde_json::json!({ "reason": error.to_string() }),
        )
    })?;
    path.starts_with(&root).then_some(path).ok_or_else(|| {
        CommandErrorVm::new(
            "conversation-directory.path-outside-root",
            serde_json::json!({ "path": input.relative_path }),
        )
    })
}

#[tauri::command]
pub async fn list_conversation_directory(
    state: State<'_, DesktopState>,
    input: ConversationDirectoryInput,
) -> CommandResult<Vec<crate::workspace_files::WorkspaceDirectoryEntryVm>> {
    let path = conversation_directory_path(&input, state.inner())?;
    let root = conversation_directory_path(
        &ConversationDirectoryInput {
            relative_path: String::new(),
            ..input.clone()
        },
        state.inner(),
    )?;
    spawn_blocking_command(move || list_conversation_directory_entries(&root, &path)).await
}

#[tauri::command]
pub async fn open_conversation_directory_path_in_file_manager(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    input: ConversationDirectoryInput,
) -> CommandResult<()> {
    let path = conversation_directory_path(&input, state.inner())?;
    app_handle
        .opener()
        .reveal_item_in_dir(&path)
        .map_err(|error| {
            CommandErrorVm::new(
                "conversation-directory.file-manager-open-failed",
                serde_json::json!({ "reason": error.to_string() }),
            )
        })
}

#[tauri::command]
pub async fn read_conversation_directory_file(
    state: State<'_, DesktopState>,
    runtime: State<'_, crate::workspace_files::WorkspaceFileRuntime>,
    input: ConversationDirectoryInput,
) -> CommandResult<crate::workspace_files::WorkspaceFileSnapshotVm> {
    let path = conversation_directory_path(&input, state.inner())?;
    let root = conversation_directory_path(
        &ConversationDirectoryInput {
            relative_path: String::new(),
            ..input.clone()
        },
        state.inner(),
    )?;
    let project_id = input.project_id.unwrap_or_else(|| "default".to_owned());
    let runtime = runtime.inner().clone();
    spawn_blocking_command(move || {
        crate::workspace_files::read_file_from_directory_root(project_id, root, path, runtime)
    })
    .await
}

fn list_conversation_directory_entries(
    root: &Path,
    directory: &Path,
) -> CommandResult<Vec<crate::workspace_files::WorkspaceDirectoryEntryVm>> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| {
            CommandErrorVm::new(
                "conversation-directory.read-failed",
                serde_json::json!({ "reason": error.to_string() }),
            )
        })?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            let canonical_path = std::fs::canonicalize(&path).ok()?;
            let relative_path = canonical_path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let kind = if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                return None;
            };
            Some(crate::workspace_files::WorkspaceDirectoryEntryVm {
                name: entry.file_name().to_string_lossy().into_owned(),
                relative_path,
                canonical_path: canonical_path.to_string_lossy().into_owned(),
                kind: kind.to_owned(),
                has_children: metadata.is_dir(),
                byte_length: metadata.is_file().then_some(metadata.len()),
                modified_at_ns: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|time| time.as_nanos().to_string()),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        (left.kind != "directory", left.name.to_lowercase())
            .cmp(&(right.kind != "directory", right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub async fn respond_elicitation(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    elicitation_id: String,
    action: String, // "accept" | "decline"
    content: Option<serde_json::Value>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;

    // Reclaim the durable scheduled occurrence before writing the response file.
    // The ACP waiter may resume immediately after the file is visible.
    if let Ok(coordinator) = state.scheduler_coordinator() {
        coordinator
            .resume_attention(
                app.paths.repo_root.clone(),
                task_id.clone(),
                run_id.clone(),
                round_id.clone(),
                attempt_id.clone(),
            )
            .await
            .map_err(|error| command_error(anyhow::anyhow!(error.to_string())))?;
    }

    let action = match action.as_str() {
        "accept" => ElicitationAction::Accept,
        _ => ElicitationAction::Decline,
    };

    let attempt_dir = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (outer_node_id.as_deref(), outer_attempt_id.as_deref())
    {
        app.paths.dynamic_node_attempt_dir(
            &task_id,
            &run_id,
            &round_id,
            outer_node_id,
            outer_attempt_id,
            &node_id,
            &attempt_id,
        )
    } else {
        app.paths
            .attempt_dir(&task_id, &run_id, &round_id, &node_id, &attempt_id)
    };
    write_elicitation_response(
        &attempt_dir,
        &elicitation_id,
        action.clone(),
        content.clone(),
        current_timestamp(),
    )
    .map_err(command_error)?;

    // Emit session update so the frontend can refresh the timeline
    // immediately. The runtime owns consumption and cleanup of the durable
    // response signal; snapshot/session status is not proof that no waiter exists.
    let session =
        if let (Some(on), Some(oa)) = (outer_node_id.as_deref(), outer_attempt_id.as_deref()) {
            crate::view_models::dynamic_acp_session_vm(
                &app,
                &task_id,
                &run_id,
                &round_id,
                on,
                oa,
                &node_id,
                &attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                &app,
                &task_id,
                &run_id,
                &round_id,
                &node_id,
                &attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        };

    emit_acp_session_update(
        &app_handle,
        &app,
        project_id,
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outer_node_id,
        outer_attempt_id,
        session,
    );

    Ok(())
}

fn open_path(path: &std::path::Path) -> Result<(), String> {
    open::that(path).map_err(|e| format!("Failed to open path: {e}"))
}

// ── MCP Server Commands ──

#[tauri::command]
pub async fn list_mcp_servers(state: State<'_, DesktopState>) -> CommandResult<Vec<McpServerVm>> {
    let context = state.context().map_err(command_error)?;
    let health = state.mcp_health_snapshot().unwrap_or_default();
    spawn_blocking_command(move || {
        let app = context.app();
        Ok(mcp_server_list_vm(
            &app.list_mcp_servers().map_err(command_error)?,
            &health,
        ))
    })
    .await
}

#[tauri::command]
pub fn add_mcp_server(
    state: State<'_, DesktopState>,
    json_content: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
    gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
        .map_err(command_error)?;
    let health = state.mcp_health_snapshot().unwrap_or_default();
    Ok(mcp_server_list_vm(
        &app.add_mcp_server(&json_content).map_err(command_error)?,
        &health,
    ))
}

#[tauri::command]
pub fn update_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
    json_content: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
    gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
        .map_err(command_error)?;
    let health = state.mcp_health_snapshot().unwrap_or_default();
    Ok(mcp_server_list_vm(
        &app.update_mcp_server(&id, &json_content)
            .map_err(command_error)?,
        &health,
    ))
}

#[tauri::command]
pub fn delete_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
    gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
        .map_err(command_error)?;
    let health = state.mcp_health_snapshot().unwrap_or_default();
    Ok(mcp_server_list_vm(
        &app.delete_mcp_server(&id).map_err(command_error)?,
        &health,
    ))
}

#[tauri::command]
pub fn toggle_mcp_server(
    state: State<'_, DesktopState>,
    id: String,
    enabled: bool,
) -> CommandResult<Vec<McpServerVm>> {
    let app = state.app().map_err(command_error)?;
    ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
    gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
        .map_err(command_error)?;
    let health = state.mcp_health_snapshot().unwrap_or_default();
    Ok(mcp_server_list_vm(
        &app.toggle_mcp_server(&id, enabled).map_err(command_error)?,
        &health,
    ))
}

#[tauri::command]
pub async fn check_mcp_server_health(
    state: State<'_, DesktopState>,
    id: String,
) -> CommandResult<gold_band::config::McpServerHealthResult> {
    // 健康检查包含阻塞式网络/进程 I/O，必须在 spawn_blocking 中执行，
    // 否则同步 command 会卡住 webview 主线程（首次进入 MCP 管理时界面冻结的根因）。
    let settings_path = {
        let app = state.app().map_err(command_error)?;
        app.paths.user_settings_file()
    };
    let id_for_cache = id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        gold_band::mcp::McpManager::new(settings_path)
            .check_health(&id)
            .map_err(command_error)
    })
    .await
    .map_err(|e| command_error(anyhow::anyhow!("health check task failed: {e}")))??;
    // 写入共享缓存，供列表 VM 展示（手动诊断与启动后台线程共用此入口）。
    let cache_state = match result.status.as_str() {
        "healthy" => gold_band::config::McpServerState::Running {
            tools: result.tools.clone(),
        },
        "auth_required" => gold_band::config::McpServerState::AuthRequired {
            auth_url: result.auth_url.clone(),
        },
        _ => gold_band::config::McpServerState::Error {
            message: result
                .message
                .clone()
                .unwrap_or_else(|| "unknown error".into()),
        },
    };
    let _ = state.record_mcp_health(id_for_cache, cache_state);
    Ok(result)
}

#[tauri::command]
pub async fn list_mcp_tools(
    state: State<'_, DesktopState>,
    id: String,
) -> CommandResult<Vec<gold_band::config::ToolInfo>> {
    // tools/list 同样包含阻塞式 I/O（SSE 长连接 + HTTP），需放到 spawn_blocking。
    let settings_path = {
        let app = state.app().map_err(command_error)?;
        app.paths.user_settings_file()
    };
    tauri::async_runtime::spawn_blocking(move || {
        gold_band::mcp::McpManager::new(settings_path)
            .list_tools(&id)
            .map_err(command_error)
    })
    .await
    .map_err(|e| command_error(anyhow::anyhow!("list tools task failed: {e}")))?
}

// ── SKILL Commands ──

#[tauri::command]
pub async fn list_skills(state: State<'_, DesktopState>) -> CommandResult<SkillListVm> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
    })
    .await
}

#[tauri::command]
pub async fn list_project_skills(
    state: State<'_, DesktopState>,
    workspace_path: String,
) -> CommandResult<Vec<SkillMetaVm>> {
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let manager = app.skill_manager();
        let skills = manager
            .list_by_workspace(&workspace_path)
            .map_err(command_error)?;
        Ok(skills.iter().map(skill_meta_vm).collect())
    })
    .await
}

#[tauri::command]
pub fn read_skill(
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
    directory_path: Option<String>,
) -> CommandResult<SkillContentVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;

    // ???? directory_path??? agent ?????? SKILL?
    if let Some(ref dir_path) = directory_path {
        let dir = camino::Utf8PathBuf::from(dir_path);
        // ??????? agent_source: <home>/<agent_dir>/skills/<name>?agent_dir ?????
        let agent_source = dir
            .parent() // <home>/<agent_dir>/skills/
            .and_then(|p| p.parent()) // <home>/<agent_dir>/
            .and_then(|p| p.file_name()) // "<agent_dir>"
            .unwrap_or(".gold-band");
        let result = app
            .skill_manager()
            .read_by_path(&dir, &name, skill_source, agent_source);
        return Ok(skill_content_vm(&result.map_err(command_error)?));
    }

    if let Some(ref ws_path) = workspace_path {
        if skill_source == gold_band::config::SkillSource::Project {
            let dir = gold_band::skill::SkillManager::workspace_skills_dir(ws_path);
            let skill_path = dir.join(&name).join(gold_band::config::SKILL_FILE_NAME);
            let raw = std::fs::read_to_string(&skill_path)
                .map_err(|e| command_error(anyhow::anyhow!(e)))?;
            let (meta, body) = gold_band::skill::parse_skill_md_public(
                &raw,
                &name,
                skill_source,
                skill_path.as_str(),
                ".gold-band",
            );
            let description_source =
                gold_band::frontmatter::parse_optional_frontmatter_document(&raw)
                    .ok()
                    .and_then(|document| {
                        document
                            .field_sources
                            .get("description")
                            .cloned()
                            .or_else(|| document.fields.get("description").cloned())
                    })
                    .unwrap_or_else(|| meta.description.clone());
            return Ok(skill_content_vm(&gold_band::skill::SkillContent {
                meta,
                description_source,
                body,
            }));
        }
    }
    Ok(skill_content_vm(
        &app.read_skill(&name, skill_source).map_err(command_error)?,
    ))
}

#[tauri::command]
pub fn write_skill(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    content: String,
    workspace_path: Option<String>,
    old_name: Option<String>,
    directory_path: Option<String>,
    sync_targets: Option<Vec<String>>,
) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    let refresh_workspace = workspace_path
        .as_deref()
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|| app.paths.repo_root.clone());

    app.skill_manager()
        .write_instance(
            &name,
            skill_source,
            &content,
            workspace_path.as_deref(),
            old_name.as_deref(),
            directory_path.as_deref(),
            sync_targets.as_deref(),
        )
        .map_err(command_error)?;

    schedule_agent_command_catalog_refresh(app_handle, refresh_workspace);

    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

#[tauri::command]
pub fn delete_skill(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
    directory_path: Option<String>,
) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    let refresh_workspace = workspace_path
        .as_deref()
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|| app.paths.repo_root.clone());

    if let Some(ref dir_path) = directory_path {
        app.cleanup_skill_instance_links(
            &name,
            dir_path,
            skill_source,
            workspace_path.as_deref(),
            None,
        );
        let dir = Utf8PathBuf::from(dir_path);
        app.skill_manager()
            .delete_at_path(&dir)
            .map_err(command_error)?;
        schedule_agent_command_catalog_refresh(app_handle, refresh_workspace);
        return Ok(skill_list_vm(&app.list_skills().map_err(command_error)?));
    }

    if let Some(ref ws_path) = workspace_path {
        if skill_source == gold_band::config::SkillSource::Project {
            let dir = gold_band::skill::SkillManager::workspace_skills_dir(ws_path);
            let skill_dir = dir.join(&name);
            if !skill_dir.exists() {
                return Err(command_error(anyhow::anyhow!("SKILL `{name}` not found")));
            }
            app.cleanup_skill_instance_links(
                &name,
                skill_dir.as_str(),
                skill_source,
                workspace_path.as_deref(),
                None,
            );
            std::fs::remove_dir_all(skill_dir.as_std_path())
                .map_err(|e| command_error(anyhow::anyhow!(e)))?;
            schedule_agent_command_catalog_refresh(app_handle, refresh_workspace);
            return Ok(skill_list_vm(&app.list_skills().map_err(command_error)?));
        }
    }

    let source_dir = match skill_source {
        gold_band::config::SkillSource::Global => {
            gold_band::storage::GoldBandPaths::global_skills_dir().join(&name)
        }
        gold_band::config::SkillSource::Project => app.paths.project_skills_dir().join(&name),
        gold_band::config::SkillSource::BuiltIn => {
            return Err(CommandErrorVm::new(
                "skill.invalid-source",
                serde_json::json!({ "source": source }),
            ));
        }
    };
    app.cleanup_skill_instance_links(
        &name,
        source_dir.as_str(),
        skill_source,
        workspace_path.as_deref(),
        None,
    );
    app.delete_skill(&name, skill_source)
        .map_err(command_error)?;
    schedule_agent_command_catalog_refresh(app_handle, refresh_workspace);
    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

#[tauri::command]
pub fn update_skill_sync_targets(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
    directory_path: String,
    sync_targets: Vec<String>,
) -> CommandResult<SkillListVm> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    let refresh_workspace = workspace_path
        .as_deref()
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|| app.paths.repo_root.clone());
    app.reconcile_skill_instance_links(
        &name,
        &directory_path,
        skill_source,
        workspace_path.as_deref(),
        Some(sync_targets.as_slice()),
    )
    .map_err(command_error)?;
    schedule_agent_command_catalog_refresh(app_handle, refresh_workspace);
    Ok(skill_list_vm(&app.list_skills().map_err(command_error)?))
}

fn schedule_agent_command_catalog_refresh(app_handle: AppHandle, workspace: Utf8PathBuf) {
    std::thread::spawn(move || {
        let state = app_handle.state::<DesktopState>();
        let _ = state.refresh_all_agent_command_catalogs_for_workspace(workspace);
        emit_agent_commands_updated(&app_handle, None);
    });
}

/// 查询指定 SKILL 在各 agent 目录中的同步状态（软链即状态）
#[tauri::command]
pub fn get_skill_sync_status(
    state: State<'_, DesktopState>,
    _name: String,
    directory_path: String,
    workspace_path: Option<String>,
) -> CommandResult<Vec<SyncStatusEntryVm>> {
    let app = state.app().map_err(command_error)?;
    let home = gold_band::storage::GoldBandPaths::global_skills_dir()
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.as_std_path().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let src_path = std::path::Path::new(&directory_path);
    // ??????????????junction ???????????
    let canonical_src = std::fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
    let skill_dir_name =
        gold_band::skill::skill_dir_name_from_str(&directory_path).ok_or_else(|| {
            command_error(anyhow::anyhow!("invalid skill directory: {directory_path}"))
        })?;
    let mut statuses = Vec::new();

    for (agent_id, config) in &app.config.agents {
        // 检查全局 agent 目录
        let global_synced = config.primary_agent_dir.as_deref().is_some_and(|dir_name| {
            let global_link =
                gold_band::skill::resolve_agent_skills_dir(&home, dir_name).join(skill_dir_name);
            is_link_pointing_to(global_link.as_std_path(), &canonical_src)
        });

        // ????? agent ?????? workspace_path?
        let project_synced = workspace_path.as_deref().map_or(false, |ws| {
            let Some(project_dir_name) = config
                .project_primary_agent_dir
                .as_deref()
                .or(config.primary_agent_dir.as_deref())
            else {
                return false;
            };
            let project_link = gold_band::skill::resolve_agent_skills_dir(
                std::path::Path::new(ws),
                project_dir_name,
            )
            .join(skill_dir_name);
            is_link_pointing_to(project_link.as_std_path(), &canonical_src)
        });

        statuses.push(SyncStatusEntryVm {
            agent_type: agent_id.as_str().to_string(),
            is_synced: global_synced || project_synced,
        });
    }

    Ok(statuses)
}

/// ????????? expected ????
fn is_link_pointing_to(link_path: &std::path::Path, expected: &std::path::Path) -> bool {
    if !link_path.exists() {
        return false;
    }
    let Ok(target) = link_path.read_link() else {
        return false;
    };
    let canonical_target = std::fs::canonicalize(&target).unwrap_or(target);
    canonical_target == expected
}

/// ???? SKILL ???? agent ?????? SKILL ??
#[tauri::command]
pub fn check_skill_name_conflict(
    state: State<'_, DesktopState>,
    name: String,
    source: String,
    workspace_path: Option<String>,
    old_name: Option<String>,
    directory_path: Option<String>,
    sync_targets: Option<Vec<String>>,
) -> CommandResult<Vec<String>> {
    let app = state.app().map_err(command_error)?;
    let skill_source = parse_skill_source(&source)?;
    app.skill_manager()
        .check_save_conflict(
            &name,
            skill_source,
            workspace_path.as_deref(),
            old_name.as_deref(),
            directory_path.as_deref(),
            sync_targets.as_deref(),
        )
        .map_err(command_error)
}

fn parse_skill_source(source: &str) -> Result<gold_band::config::SkillSource, CommandErrorVm> {
    match source {
        "global" => Ok(gold_band::config::SkillSource::Global),
        "project" => Ok(gold_band::config::SkillSource::Project),
        "built-in" => Ok(gold_band::config::SkillSource::BuiltIn),
        _ => Err(CommandErrorVm::new(
            "skill.invalid-source",
            serde_json::json!({ "source": source }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use gold_band::storage::write_json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn blocking_command_runs_outside_the_caller_thread() {
        let caller_thread = std::thread::current().id();

        let worker_thread = tauri::async_runtime::block_on(async {
            spawn_blocking_command(|| Ok(std::thread::current().id()))
                .await
                .unwrap()
        });

        assert_ne!(worker_thread, caller_thread);
    }

    #[test]
    fn acp_session_update_serializes_lightweight_prompt_activity_and_terminal_clear() {
        let active = AcpSessionUpdatedEventVm {
            branch_id: None,
            project_id: Some("project-a".to_string()),
            task_id: "task-a".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
            session: None,
            event: None,
            lifecycle: None,
            activity: Some(conversation_task_activity_from_prompt(
                client::PromptActivity::Running,
            )),
        };
        let active_json = serde_json::to_value(active).unwrap();
        assert_eq!(
            active_json["activity"],
            serde_json::json!({ "phase": "running", "stopping": false })
        );
        assert!(active_json["lifecycle"].is_null());

        let terminal = AcpSessionUpdatedEventVm {
            branch_id: None,
            project_id: Some("project-a".to_string()),
            task_id: "task-a".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
            session: None,
            event: None,
            lifecycle: None,
            activity: None,
        };
        assert!(serde_json::to_value(terminal).unwrap()["activity"].is_null());
    }

    #[test]
    fn accepted_stop_persists_control_state_without_reading_timeline() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-stop-control-test-{}",
            uuid::Uuid::new_v4()
        ));
        let repo_root = Utf8PathBuf::from_path_buf(root.clone()).unwrap();
        let app = App::new(repo_root);
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        write_json(
            &app.paths.run_file("task-001", "run-001"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "id": "run-001",
                "task_id": "task-001",
                "status": "running",
                "outcome": null,
                "started_at": "2026-08-05T00:00:00Z",
                "updated_at": "2026-08-05T00:00:00Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": "round-001",
                "current_node": "node-001",
                "current_attempt": "attempt-001",
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        write_json(
            &app.paths.round_file("task-001", "run-001", "round-001"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "id": "round-001",
                "run_id": "run-001",
                "index": 1,
                "status": "running",
                "outcome": null,
                "trigger": "initial",
                "started_at": "2026-08-05T00:00:00Z",
                "trace": []
            }),
        )
        .unwrap();
        write_json(
            &app.paths.node_file(
                "task-001",
                "run-001",
                "round-001",
                "node-001",
                "attempt-001",
            ),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "node_id": "node-001",
                "node_type": "worker",
                "run_id": "run-001",
                "round_id": "round-001",
                "attempt_id": "attempt-001",
                "status": "running",
                "outcome": null,
                "started_at": "2026-08-05T00:00:00Z",
                "finished_at": null,
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();
        let timeline_path = app.paths.acp_timeline_file(
            "task-001",
            "run-001",
            "round-001",
            "node-001",
            "attempt-001",
        );
        std::fs::create_dir_all(timeline_path.as_std_path()).unwrap();

        let started = std::time::Instant::now();
        let attempt_dir = persist_active_session_stop(&app, &locator).unwrap();

        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let run: serde_json::Value = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        assert_eq!(run["status"], "paused");
        let snapshot: serde_json::Value =
            read_json(&attempt_dir.join("acp.snapshot.json")).unwrap();
        assert_eq!(snapshot["status"], "cancelled");
        assert!(timeline_path.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn branch_query_errors_keep_their_structured_command_code() {
        let error = command_error(
            gold_band::acp::branches::ConversationBranchError::InvalidBranchId.into(),
        );
        assert_eq!(error.code, "acp.invalid-conversation-branch-id");
        assert_eq!(error.params, serde_json::json!({}));
    }

    #[test]
    fn runtime_errors_keep_their_structured_command_code_and_params() {
        let error = command_error(gold_band::runtime_error::runtime_error(
            gold_band::runtime_error::blocked_runtime_error_info(
                gold_band::runtime_error::RuntimeErrorDomain::Provider,
                gold_band::acp::client::ACP_SESSION_RESTORE_UNSUPPORTED_CODE,
                "internal diagnostic only",
                serde_json::json!({ "capabilities": { "resume": false, "load": false } }),
            ),
        ));

        assert_eq!(error.code, "acp.session-restore-unsupported");
        assert_eq!(
            error.params,
            serde_json::json!({ "capabilities": { "resume": false, "load": false } })
        );
    }

    #[test]
    fn storage_query_errors_do_not_expose_backend_messages() {
        let error = acp_storage_query_error(
            anyhow::anyhow!("D:/secret/path could not be parsed"),
            "acp.activity-detail-query-failed",
        );
        assert_eq!(error.code, "acp.activity-detail-query-failed");
        assert_eq!(error.params, serde_json::json!({}));
    }

    #[test]
    fn app_exit_warnings_keep_codes_without_exposing_backend_messages() {
        let mut result = AppExitPreparationVm::default();
        result.record_warning(
            "app-exit.session-stop-failed",
            &anyhow::anyhow!("D:/secret/provider process failed"),
        );

        let value = serde_json::to_value(result).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "warnings": [{
                    "code": "app-exit.session-stop-failed",
                    "params": {}
                }]
            })
        );
        assert!(!value.to_string().contains("secret"));
    }

    #[test]
    fn managed_agent_input_preserves_user_editable_fields_and_uses_internal_capability() {
        let input = ManagedAgentInput {
            display_name: "Claude Custom".to_string(),
            icon: String::new(),
            command: "  npx  ".to_string(),
            args: vec!["agent".to_string()],
            env: std::collections::BTreeMap::from([("TOKEN".to_string(), "secret".to_string())]),
            primary_agent_dir: "  .claude-custom  ".to_string(),
            project_primary_agent_dir: Some("  .claude-project  ".to_string()),
            compatible_agent_dirs: vec![
                " .agents ".to_string(),
                ".agents".to_string(),
                " ".to_string(),
            ],
            external_session_sync_supported: false,
            external_session_sync_enabled: true,
        };

        let config = input
            .into_config(
                gold_band::config::SystemPromptDelivery::MetaAppend,
                DEFAULT_CUSTOM_AGENT_ICON,
            )
            .unwrap();
        assert_eq!(config.adapter.display_name, "Claude Custom");
        assert_eq!(config.adapter.command, "npx");
        assert_eq!(config.icon, DEFAULT_CUSTOM_AGENT_ICON);
        assert_eq!(config.primary_agent_dir.as_deref(), Some(".claude-custom"));
        assert_eq!(
            config.project_primary_agent_dir.as_deref(),
            Some(".claude-project")
        );
        assert_eq!(config.compatible_agent_dirs, vec![".agents"]);
        assert!(config.supports_system_prompt());
        assert!(!config.external_session_sync_enabled);
    }

    #[test]
    fn managed_agent_input_cannot_override_internal_system_prompt_capability() {
        let input: ManagedAgentInput = serde_json::from_value(serde_json::json!({
            "displayName": "Custom Agent",
            "icon": "agent",
            "command": "custom-acp",
            "supportsSystemPrompt": true
        }))
        .unwrap();

        let config = input
            .into_config(
                gold_band::config::SystemPromptDelivery::None,
                DEFAULT_CUSTOM_AGENT_ICON,
            )
            .unwrap();

        assert!(!config.supports_system_prompt());
        assert_eq!(
            system_prompt_delivery_for_new_agent(
                &ManagedAgentId::from_str("custom-agent").unwrap()
            ),
            gold_band::config::SystemPromptDelivery::None
        );
        assert_eq!(
            system_prompt_delivery_for_new_agent(&ManagedAgentId::from_str("claude-acp").unwrap()),
            gold_band::config::SystemPromptDelivery::MetaAppend
        );
        assert_eq!(
            default_icon_for_agent(&ManagedAgentId::from_str("claude-acp").unwrap()),
            "claude"
        );
        assert_eq!(
            default_icon_for_agent(&ManagedAgentId::from_str("custom-agent").unwrap()),
            DEFAULT_CUSTOM_AGENT_ICON
        );
    }

    #[test]
    fn managed_agent_input_requires_display_name_and_command_at_the_command_boundary() {
        let missing_name = ManagedAgentInput {
            display_name: "  ".to_string(),
            icon: String::new(),
            command: "agent-acp".to_string(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
            primary_agent_dir: String::new(),
            project_primary_agent_dir: None,
            compatible_agent_dirs: Vec::new(),
            external_session_sync_supported: false,
            external_session_sync_enabled: false,
        }
        .into_config(
            gold_band::config::SystemPromptDelivery::None,
            DEFAULT_CUSTOM_AGENT_ICON,
        )
        .unwrap_err();
        assert_eq!(missing_name.code, "agent.display-name-required");

        let missing_command = ManagedAgentInput {
            display_name: "Custom Agent".to_string(),
            icon: String::new(),
            command: "  ".to_string(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
            primary_agent_dir: String::new(),
            project_primary_agent_dir: None,
            compatible_agent_dirs: Vec::new(),
            external_session_sync_supported: false,
            external_session_sync_enabled: false,
        }
        .into_config(
            gold_band::config::SystemPromptDelivery::None,
            DEFAULT_CUSTOM_AGENT_ICON,
        )
        .unwrap_err();
        assert_eq!(missing_command.code, "agent.command-required");
    }

    #[test]
    fn acp_follow_up_uses_only_gold_band_model_override() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-model-override-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "models": { "currentModelId": "default" },
                "configOptions": [
                    { "id": "model", "currentValue": "default" }
                ]
            }),
        )
        .unwrap();
        assert_eq!(current_acp_session_model_override(&attempt_dir), None);

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "modelOverride": "default",
                "models": { "currentModelId": "default" }
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_model_override(&attempt_dir).as_deref(),
            Some("default")
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn acp_follow_up_uses_only_gold_band_permission_mode_override() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-permission-mode-override-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "modes": { "currentModeId": "default" },
                "configOptions": [
                    { "id": "mode", "currentValue": "default" }
                ]
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_permission_mode_override(&attempt_dir),
            None
        );

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "permissionModeOverride": "default",
                "modes": { "currentModeId": "default" }
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_permission_mode_override(&attempt_dir).as_deref(),
            Some("default")
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn acp_follow_up_reads_generic_config_option_overrides_by_actual_id() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-config-option-override-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "configOptionOverrides": {
                    "reasoning_effort": "high"
                },
                "configOptions": [{
                    "id": "reasoning_effort",
                    "category": "thought_level",
                    "type": "select",
                    "currentValue": "high"
                }]
            }),
        )
        .unwrap();

        assert_eq!(
            current_acp_session_config_option_overrides(&attempt_dir),
            std::collections::BTreeMap::from([(
                "reasoning_effort".to_string(),
                "high".to_string(),
            )])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn acp_turn_outcome_distinguishes_user_cancel_from_transport_failure() {
        assert_eq!(
            acp_turn_outcome_for_stop_reason(Some("cancelled")),
            AcpTurnOutcome::Cancelled
        );
        assert_eq!(
            acp_turn_outcome_for_stop_reason(Some("canceled")),
            AcpTurnOutcome::Cancelled
        );
        assert_eq!(
            acp_turn_outcome_for_stop_reason(Some("interrupted")),
            AcpTurnOutcome::Failed
        );
        assert_eq!(
            acp_turn_outcome_for_stop_reason(Some("end_turn")),
            AcpTurnOutcome::Completed
        );
        assert_eq!(
            acp_turn_outcome_for_stop_reason(None),
            AcpTurnOutcome::Completed
        );
    }

    #[test]
    fn acp_turn_finished_event_preserves_turn_identity_outcome_and_batch_continuation() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-acp-turn-event-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(root.clone()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_subscriber = seen.clone();
        let app = App::new(repo_root).with_inline_lifecycle_subscriber(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::AcpTurnFinished { .. } = event {
                seen_for_subscriber.lock().unwrap().push(event);
            }
        }));
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );

        emit_acp_turn_finished(
            &app,
            &locator,
            "turn-002",
            "Claude",
            AcpTurnOutcome::Failed,
            AcpTurnBatchProgress::terminal(1),
        );
        emit_deferred_turn_completion(
            &app,
            &locator,
            Some(&DeferredTurnCompletion {
                turn_id: "acp-prompt-003".to_string(),
                agent_label: "Claude".to_string(),
            }),
            true,
        );
        emit_deferred_turn_completion(
            &app,
            &locator,
            Some(&DeferredTurnCompletion {
                turn_id: "acp-prompt-004".to_string(),
                agent_label: "Claude".to_string(),
            }),
            false,
        );

        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 3);
        match &events[0] {
            RuntimeLifecycleEvent::AcpTurnFinished {
                event_id,
                turn_id,
                agent_label,
                outcome,
                batch_progress,
                ..
            } => {
                assert_eq!(
                    event_id,
                    &format!(
                        "{}:run-001:round-001:node-001:attempt-001:turn-002:acp-turn-finished",
                        app.paths.project_id
                    )
                );
                assert_eq!(turn_id, "turn-002");
                assert_eq!(agent_label, "Claude");
                assert_eq!(*outcome, AcpTurnOutcome::Failed);
                assert_eq!(*batch_progress, AcpTurnBatchProgress::terminal(1));
            }
            event => panic!("expected AcpTurnFinished, got {event:?}"),
        }
        match &events[1] {
            RuntimeLifecycleEvent::AcpTurnFinished {
                turn_id,
                outcome,
                batch_progress,
                ..
            } => {
                assert_eq!(turn_id, "acp-prompt-003");
                assert_eq!(*outcome, AcpTurnOutcome::Completed);
                assert_eq!(batch_progress.completed_reply_count, 1);
                assert!(batch_progress.continues);
            }
            event => panic!("expected deferred AcpTurnFinished, got {event:?}"),
        }
        match &events[2] {
            RuntimeLifecycleEvent::AcpTurnFinished {
                turn_id,
                outcome,
                batch_progress,
                ..
            } => {
                assert_eq!(turn_id, "acp-prompt-004");
                assert_eq!(*outcome, AcpTurnOutcome::Completed);
                assert_eq!(*batch_progress, AcpTurnBatchProgress::terminal(2));
            }
            event => panic!("expected terminal AcpTurnFinished, got {event:?}"),
        }
        drop(events);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scheduled_acp_preflight_failure_emits_one_failed_turn() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-scheduled-acp-preflight-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(root.clone()).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_subscriber = seen.clone();
        let app = App::new(repo_root)
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_inline_lifecycle_subscriber(Arc::new(move |event| {
                if let RuntimeLifecycleEvent::AcpTurnFinished { .. } = event {
                    seen_for_subscriber.lock().unwrap().push(event);
                }
            }));
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );

        let result: CommandResult<()> = finish_acp_prompt_preflight(
            &app,
            &locator,
            "turn-001",
            "Claude",
            Err(CommandErrorVm::new(
                "runtime.conversation-not-available",
                serde_json::json!({}),
            )),
        );

        assert_eq!(
            result.unwrap_err().code,
            "runtime.conversation-not-available"
        );
        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            RuntimeLifecycleEvent::AcpTurnFinished {
                scheduled_occurrence_id,
                turn_id,
                outcome,
                ..
            } => {
                assert_eq!(scheduled_occurrence_id.as_deref(), Some("occurrence-001"));
                assert_eq!(turn_id, "turn-001");
                assert_eq!(*outcome, AcpTurnOutcome::Failed);
            }
            event => panic!("expected AcpTurnFinished, got {event:?}"),
        }
        drop(events);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acp_live_event_context_preserves_standard_attempt_locator() {
        let context = acp_live_event_context(
            "task-001",
            "run-001",
            "round-001",
            "dev",
            "attempt-001",
            None,
            None,
        );

        assert_eq!(context.task_id, "task-001");
        assert_eq!(context.run_id, "run-001");
        assert_eq!(context.round_id, "round-001");
        assert_eq!(context.node_id, "dev");
        assert_eq!(context.attempt_id, "attempt-001");
        assert_eq!(context.outer_node_id, None);
        assert_eq!(context.outer_attempt_id, None);
    }

    #[test]
    fn acp_live_event_context_preserves_dynamic_attempt_locator() {
        let context = acp_live_event_context(
            "task-001",
            "run-001",
            "round-001",
            "bootstrap",
            "attempt-002",
            Some("ai-dynamic".to_string()),
            Some("attempt-001".to_string()),
        );

        assert_eq!(context.node_id, "bootstrap");
        assert_eq!(context.attempt_id, "attempt-002");
        assert_eq!(context.outer_node_id.as_deref(), Some("ai-dynamic"));
        assert_eq!(context.outer_attempt_id.as_deref(), Some("attempt-001"));
    }

    #[test]
    fn conversation_run_state_update_maps_paused_and_completed_events() {
        let paused = conversation_run_state_update_for_event(RuntimeLifecycleEvent::RunPaused {
            event_id: "event-paused".to_string(),
            occurred_at: "2026-06-25T00:00:00Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-1".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_label: "plan".to_string(),
            pause_reason: PauseReason::WaitingForUserInput,
            task_title: None,
        })
        .unwrap();
        assert_eq!(paused.project_id, "project-1");
        assert_eq!(paused.task_id, "task-001");
        assert_eq!(paused.run_id, "run-001");
        assert_eq!(paused.round_id, "round-001");
        assert_eq!(paused.node_id, "plan");
        assert_eq!(paused.attempt_id, "attempt-001");
        assert_eq!(paused.status, RunStatus::Paused);
        assert_eq!(paused.outcome, None);

        let completed =
            conversation_run_state_update_for_event(RuntimeLifecycleEvent::RunCompleted {
                event_id: "event-completed".to_string(),
                occurred_at: "2026-06-25T00:00:01Z".to_string(),
                scheduled_occurrence_id: None,
                project_id: "project-1".to_string(),
                task_id: "task-001".to_string(),
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                node_id: "plan".to_string(),
                attempt_id: "attempt-001".to_string(),
                node_label: "plan".to_string(),
                outcome: RunOutcome::Success,
                task_title: None,
                completion_agent_label: None,
            })
            .unwrap();
        assert_eq!(completed.status, RunStatus::Completed);
        assert_eq!(completed.outcome, Some(RunOutcome::Success));
    }

    #[test]
    fn fixed_runtime_continue_rejects_structured_intervention_states() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-runtime-continue-eligibility-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let mut run = RunState {
            version: gold_band::domain::VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-10T00:00:00Z".to_string(),
            updated_at: "2026-08-10T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("node-001".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
        };

        assert!(runtime_continue_required(&app, &locator, &run, false).unwrap());
        run.pause_reason = Some(PauseReason::RuntimeAbnormal);
        assert!(runtime_continue_required(&app, &locator, &run, false).unwrap());
        assert!(!runtime_continue_required(&app, &locator, &run, true).unwrap());

        for reason in [
            PauseReason::WaitingForUserInput,
            PauseReason::PermissionRequested,
            PauseReason::ErrorBlocked,
        ] {
            run.pause_reason = Some(reason);
            assert!(!runtime_continue_required(&app, &locator, &run, false).unwrap());
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stopped_workflow_allows_conversation_without_consuming_runtime_continue() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-stopped-workflow-conversation-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        write_json(
            &app.paths
                .task_dir("task-001")
                .join("authoring")
                .join("conversation.json"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "source": "conversation",
                "runMode": "workflow",
                "workflowTemplateId": "default",
                "includeInterview": true,
                "directConfig": null,
                "agentIdentity": null,
                "titleAutoGenerated": false,
                "initialAttachmentNames": null,
                "createdAt": "2026-08-12T00:00:00Z",
                "lastActivityAt": null
            }),
        )
        .unwrap();
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        write_json(
            &app.paths.node_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &NodeState {
                version: gold_band::domain::VERSION.to_string(),
                node_id: locator.node_id.clone(),
                node_type: gold_band::domain::NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: RunStatus::Paused,
                outcome: None,
                started_at: "2026-08-12T00:00:00Z".to_string(),
                finished_at: Some("2026-08-12T00:00:01Z".to_string()),
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: gold_band::domain::ResolvedConfig::new(),
                uuid: None,
            },
        )
        .unwrap();
        let run = RunState {
            version: gold_band::domain::VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-12T00:00:00Z".to_string(),
            updated_at: "2026-08-12T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(locator.node_id.clone()),
            current_attempt: Some(locator.attempt_id.clone()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
        };

        assert!(runtime_continue_required(&app, &locator, &run, false).unwrap());
        assert!(!attempt_is_runtime_controlled(&app, &locator).unwrap());
        assert!(ensure_conversation_prompt_available(&app, &locator).is_ok());

        let persisted_node: NodeState = read_json(&app.paths.node_file(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        ))
        .unwrap();
        assert_eq!(persisted_node.status, RunStatus::Paused);
        assert_eq!(persisted_node.outcome, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_runtime_never_requires_explicit_workflow_continue() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-direct-runtime-continue-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        write_json(
            &app.paths
                .task_dir("task-001")
                .join("authoring")
                .join("conversation.json"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "source": "conversation",
                "runMode": "direct",
                "workflowTemplateId": null,
                "includeInterview": null,
                "directConfig": null,
                "agentIdentity": null,
                "titleAutoGenerated": false,
                "initialAttachmentNames": null,
                "createdAt": "2026-08-12T00:00:00Z",
                "lastActivityAt": null
            }),
        )
        .unwrap();
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "node-001".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let run = RunState {
            version: gold_band::domain::VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-12T00:00:00Z".to_string(),
            updated_at: "2026-08-12T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some("round-001".to_string()),
            current_node: Some("node-001".to_string()),
            current_attempt: Some("attempt-001".to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
        };

        assert!(!runtime_continue_required(&app, &locator, &run, false).unwrap());
        assert!(!attempt_is_runtime_controlled(&app, &locator).unwrap());
        assert!(ensure_conversation_prompt_available(&app, &locator).is_ok());
        assert!(!gold_band::config::ConversationRunMode::Direct.is_orchestrated());
        assert!(gold_band::config::ConversationRunMode::Auto.is_orchestrated());
        assert!(gold_band::config::ConversationRunMode::Workflow.is_orchestrated());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paused_stale_cancelled_dynamic_leaf_requires_runtime_continue() {
        let temp = std::env::temp_dir().join(format!(
            "gold-band-stale-dynamic-leaf-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        let repo_root = Utf8PathBuf::from_path_buf(temp.join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = App::new(repo_root);
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let outer_node_id = "ai-dynamic";
        let outer_attempt_id = "attempt-001";
        let dynamic_node_id = "bootstrap";
        let dynamic_attempt_id = "attempt-001";
        let run = RunState {
            version: gold_band::domain::VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Paused,
            outcome: None,
            started_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some(round_id.to_string()),
            current_node: Some(outer_node_id.to_string()),
            current_attempt: Some(outer_attempt_id.to_string()),
            new_rounds_opened: 0,
            pause_reason: Some(PauseReason::ProcessInterrupted),
            uuid: None,
            last_executed_node: None,
        };
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
        let dynamic_node = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": dynamic_node_id,
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": "Bootstrap",
            "task": "Bootstrap",
            "status": "running",
            "outcome": null,
            "groupId": null,
            "chainId": dynamic_node_id,
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "provider": "claude-acp",
            "profile": null,
            "permissionMode": null,
            "model": null,
            "sessionMode": "new",
            "continueFromNodeId": null,
            "workflowId": null,
            "workflowSnapshotId": null,
            "childRunId": null,
            "startedAt": "2026-06-16T00:00:00Z",
            "finishedAt": null
        });
        let dynamic_run = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": "dynamic-run-001",
            "parentRunId": run_id,
            "parentRoundId": round_id,
            "parentNodeId": outer_node_id,
            "parentAttemptId": outer_attempt_id,
            "status": "paused",
            "outcome": null,
            "pauseReason": "process-interrupted",
            "startedAt": "2026-06-16T00:00:00Z",
            "updatedAt": "2026-06-16T00:00:01Z",
            "control": {},
            "allowedWorkflowSnapshots": [],
            "currentNodeIds": [dynamic_node_id]
        });
        write_json(
            &app.paths.dynamic_graph_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
            ),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "run": dynamic_run,
                "nodes": [dynamic_node.clone()],
                "groups": [],
                "workspaces": [{
                    "version": gold_band::domain::VERSION,
                    "id": "workspace-main",
                    "dynamicRunId": "dynamic-run-001",
                    "kind": "main",
                    "ownership": "user",
                    "repoRoot": app.paths.repo_root,
                    "path": app.paths.repo_root,
                    "branch": null,
                    "parentWorkspaceId": null,
                    "createdByGroupId": null,
                    "forkCommit": "test-head",
                    "checkpointCommit": null,
                    "status": "active",
                    "createdAt": "2026-06-16T00:00:00Z",
                    "updatedAt": "2026-06-16T00:00:00Z"
                }],
                "proposals": []
            }),
        )
        .unwrap();
        write_json(
            &app.paths.dynamic_node_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                dynamic_node_id,
            ),
            &dynamic_node,
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    dynamic_node_id,
                    dynamic_attempt_id,
                )
                .join("acp.session.json"),
            &serde_json::json!({
                "status": "cancelled",
                "stopReason": "cancelled",
                "sessionId": "session-bootstrap"
            }),
        )
        .unwrap();
        let locator = AttemptLocator::new(
            task_id.to_string(),
            run_id.to_string(),
            round_id.to_string(),
            dynamic_node_id.to_string(),
            dynamic_attempt_id.to_string(),
            Some(outer_node_id.to_string()),
            Some(outer_attempt_id.to_string()),
        );

        assert!(runtime_continue_required(&app, &locator, &run, false).unwrap());
        let mut waiting_run = run.clone();
        waiting_run.pause_reason = Some(PauseReason::WaitingForUserInput);
        assert!(!runtime_continue_required(&app, &locator, &waiting_run, false).unwrap());
        let graph_path = app.paths.dynamic_graph_file(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
        );
        let mut blocked_graph: DynamicGraphState = read_json(&graph_path).unwrap();
        blocked_graph.run.pause_reason = Some(PauseReason::ErrorBlocked);
        write_json(&graph_path, &blocked_graph).unwrap();
        assert!(!runtime_continue_required(&app, &locator, &run, false).unwrap());
        let dynamic_run = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": "dynamic-run-001",
            "parentRunId": run_id,
            "parentRoundId": round_id,
            "parentNodeId": outer_node_id,
            "parentAttemptId": outer_attempt_id,
            "status": "running",
            "outcome": null,
            "pauseReason": null,
            "startedAt": "2026-06-16T00:00:00Z",
            "updatedAt": "2026-06-16T00:00:01Z",
            "control": {},
            "allowedWorkflowSnapshots": [],
            "currentNodeIds": [dynamic_node_id]
        });
        write_json(
            &app.paths.dynamic_graph_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
            ),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "run": dynamic_run,
                "nodes": [dynamic_node],
                "groups": [],
                "workspaces": [{
                    "version": gold_band::domain::VERSION,
                    "id": "workspace-main",
                    "dynamicRunId": "dynamic-run-001",
                    "kind": "main",
                    "ownership": "user",
                    "repoRoot": app.paths.repo_root,
                    "path": app.paths.repo_root,
                    "branch": null,
                    "parentWorkspaceId": null,
                    "createdByGroupId": null,
                    "forkCommit": "test-head",
                    "checkpointCommit": null,
                    "status": "active",
                    "createdAt": "2026-06-16T00:00:00Z",
                    "updatedAt": "2026-06-16T00:00:00Z"
                }],
                "proposals": []
            }),
        )
        .unwrap();
        assert!(runtime_continue_required(&app, &locator, &run, false).unwrap());
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn running_stale_cancelled_dynamic_leaf_does_not_require_runtime_continue() {
        let temp = std::env::temp_dir().join(format!(
            "gold-band-running-stale-dynamic-leaf-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        let repo_root = Utf8PathBuf::from_path_buf(temp.join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = App::new(repo_root);
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let outer_node_id = "ai-dynamic";
        let outer_attempt_id = "attempt-001";
        let dynamic_node_id = "bootstrap";
        let dynamic_attempt_id = "attempt-001";
        let run = RunState {
            version: gold_band::domain::VERSION.to_string(),
            id: run_id.to_string(),
            task_id: task_id.to_string(),
            task_uuid: None,
            status: RunStatus::Running,
            outcome: None,
            started_at: "2026-06-16T00:00:00Z".to_string(),
            updated_at: "2026-06-16T00:00:01Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: Some(round_id.to_string()),
            current_node: Some(outer_node_id.to_string()),
            current_attempt: Some(outer_attempt_id.to_string()),
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
        };
        write_json(&app.paths.run_file(task_id, run_id), &run).unwrap();
        let dynamic_node = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": dynamic_node_id,
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": "Bootstrap",
            "task": "Bootstrap",
            "status": "running",
            "outcome": null,
            "groupId": null,
            "chainId": dynamic_node_id,
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "provider": "claude-acp",
            "profile": null,
            "permissionMode": null,
            "model": null,
            "sessionMode": "new",
            "continueFromNodeId": null,
            "workflowId": null,
            "workflowSnapshotId": null,
            "childRunId": null,
            "startedAt": "2026-06-16T00:00:00Z",
            "finishedAt": null
        });
        let dynamic_run = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": "dynamic-run-001",
            "parentRunId": run_id,
            "parentRoundId": round_id,
            "parentNodeId": outer_node_id,
            "parentAttemptId": outer_attempt_id,
            "status": "running",
            "outcome": null,
            "pauseReason": null,
            "startedAt": "2026-06-16T00:00:00Z",
            "updatedAt": "2026-06-16T00:00:01Z",
            "control": {},
            "allowedWorkflowSnapshots": [],
            "currentNodeIds": [dynamic_node_id]
        });
        write_json(
            &app.paths.dynamic_graph_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
            ),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "run": dynamic_run,
                "nodes": [dynamic_node.clone()],
                "groups": [],
                "workspaces": [{
                    "version": gold_band::domain::VERSION,
                    "id": "workspace-main",
                    "dynamicRunId": "dynamic-run-001",
                    "kind": "main",
                    "ownership": "user",
                    "repoRoot": app.paths.repo_root,
                    "path": app.paths.repo_root,
                    "branch": null,
                    "parentWorkspaceId": null,
                    "createdByGroupId": null,
                    "forkCommit": "test-head",
                    "checkpointCommit": null,
                    "status": "active",
                    "createdAt": "2026-06-16T00:00:00Z",
                    "updatedAt": "2026-06-16T00:00:00Z"
                }],
                "proposals": []
            }),
        )
        .unwrap();
        write_json(
            &app.paths.dynamic_node_file(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                dynamic_node_id,
            ),
            &dynamic_node,
        )
        .unwrap();
        write_json(
            &app.paths
                .dynamic_node_attempt_dir(
                    task_id,
                    run_id,
                    round_id,
                    outer_node_id,
                    outer_attempt_id,
                    dynamic_node_id,
                    dynamic_attempt_id,
                )
                .join("acp.session.json"),
            &serde_json::json!({
                "status": "cancelled",
                "stopReason": "cancelled",
                "sessionId": "session-bootstrap"
            }),
        )
        .unwrap();
        let locator = AttemptLocator::new(
            task_id.to_string(),
            run_id.to_string(),
            round_id.to_string(),
            dynamic_node_id.to_string(),
            dynamic_attempt_id.to_string(),
            Some(outer_node_id.to_string()),
            Some(outer_attempt_id.to_string()),
        );

        assert!(!runtime_continue_required(&app, &locator, &run, false).unwrap());
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn providers_for_ai_dynamic_include_available_agent_providers() {
        let node = NodeDsl::AiDynamic(gold_band::dsl::AiDynamicNode {
            id: "route".to_string(),
            agent_strategy: AiDynamicAgentStrategy::Dynamic {
                bootstrap_provider: "claude-acp".to_string(),
                bootstrap_model: None,
                permission_mode: None,
                bootstrap_config_options: Default::default(),
                acceptance_model: None,
                acceptance_config_options: Default::default(),
                routing_prompt: "route by task".to_string(),
                available_agents: vec![
                    gold_band::dsl::DynamicAgentRef {
                        provider: "codex-acp".to_string(),
                        model: None,
                        permission_mode: None,
                        config_options: Default::default(),
                    },
                    gold_band::dsl::DynamicAgentRef {
                        provider: "claude-acp".to_string(),
                        model: None,
                        permission_mode: None,
                        config_options: Default::default(),
                    },
                ],
            },
            config_options: Default::default(),
            allowed_profiles: Vec::new(),
            global_goal: None,
            control: gold_band::dsl::DynamicControlDsl::default(),
            allowed_workflows: Vec::new(),
        });

        assert_eq!(
            providers_for_node(&node),
            vec!["claude-acp".to_string(), "codex-acp".to_string()]
        );
    }

    #[test]
    fn canonical_permission_request_id_maps_display_id_to_pending_file_id() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-permission-id-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        gold_band::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            serde_json::json!({}),
            "1778771541Z".to_string(),
        )
        .unwrap();

        assert_eq!(
            canonical_permission_request_id(&attempt_dir, "permission-permission-0"),
            "0"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn permission_response_signal_is_kept_for_live_waiter_when_snapshot_is_cancelled() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-permission-response-signal-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        gold_band::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            serde_json::json!({ "sessionId": "session-1" }),
            "1778771541Z".to_string(),
        )
        .unwrap();
        gold_band::storage::write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "sessionId": "session-1",
                "status": "cancelled",
                "stopReason": "cancelled"
            }),
        )
        .unwrap();

        let written = write_acp_permission_response_signal(
            &attempt_dir,
            "permission-0",
            Some("allow".into()),
        )
        .unwrap();

        assert!(written);
        assert!(
            gold_band::acp::permission::permission_response_file(&attempt_dir, "0").exists(),
            "permission response must remain for the live ACP waiter"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn maybe_emit_elicitation_intervention_for_pending_request() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-direct-intervention-label-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        write_json(
            &app.paths
                .task_dir("task-001")
                .join("authoring")
                .join("conversation.json"),
            &serde_json::json!({
                "runMode": "direct",
                "agentIdentity": { "displayName": "Claude" },
                "directConfig": { "agentType": "claude-acp" }
            }),
        )
        .unwrap();
        let bus = gold_band::app::observability::RuntimeLifecycleBus::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::InterventionRequested {
                kind,
                event_id,
                node_label,
                ..
            } = event
            {
                seen_for_handler
                    .lock()
                    .unwrap()
                    .push((kind, event_id, node_label));
            }
        }));

        maybe_emit_elicitation_intervention(
            &bus,
            &app.paths.project_id,
            Some(&app),
            &gold_band::app::AcpLiveEventContext {
                task_id: "task-001".to_string(),
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                node_id: "plan".to_string(),
                attempt_id: "attempt-001".to_string(),
                outer_node_id: None,
                outer_attempt_id: None,
            },
            &AcpUiEvent {
                kind: "elicitationRequest".to_string(),
                id: "elicit-001".to_string(),
                seq: 1,
                timestamp: "1Z".to_string(),
                session_id: None,
                status: Some("pending".to_string()),
                title: None,
                content: None,
                tool_call_id: None,
                started_seq: None,
                ended_seq: None,
                started_at: Some("1Z".to_string()),
                ended_at: None,
                timing: None,
                raw: None,
            },
        );
        maybe_emit_elicitation_intervention(
            &bus,
            &app.paths.project_id,
            Some(&app),
            &gold_band::app::AcpLiveEventContext {
                task_id: "task-001".to_string(),
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                node_id: "plan".to_string(),
                attempt_id: "attempt-001".to_string(),
                outer_node_id: None,
                outer_attempt_id: None,
            },
            &AcpUiEvent {
                kind: "elicitationRequest".to_string(),
                id: "elicit-002".to_string(),
                seq: 2,
                timestamp: "2Z".to_string(),
                session_id: None,
                status: Some("pending".to_string()),
                title: None,
                content: None,
                tool_call_id: None,
                started_seq: None,
                ended_seq: None,
                started_at: Some("2Z".to_string()),
                ended_at: None,
                timing: None,
                raw: None,
            },
        );

        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, RuntimeInterventionKind::ElicitationRequested);
        assert_eq!(
            events[0].1,
            format!(
                "{}:run-001:round-001:plan:attempt-001:elicitation-requested:elicit-001",
                app.paths.project_id
            )
        );
        assert_eq!(
            events[1].1,
            format!(
                "{}:run-001:round-001:plan:attempt-001:elicitation-requested:elicit-002",
                app.paths.project_id
            )
        );
        assert_eq!(events[0].2, "Claude");
        assert_eq!(events[1].2, "Claude");
        drop(events);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn maybe_emit_permission_intervention_keeps_requests_distinct() {
        let bus = gold_band::app::observability::RuntimeLifecycleBus::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::InterventionRequested { kind, event_id, .. } = event {
                seen_for_handler.lock().unwrap().push((kind, event_id));
            }
        }));
        let context = gold_band::app::AcpLiveEventContext {
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let permission_event = |id: &str, seq: u64| AcpUiEvent {
            kind: "permissionRequest".to_string(),
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            session_id: None,
            status: Some("pending".to_string()),
            title: None,
            content: None,
            tool_call_id: None,
            started_seq: None,
            ended_seq: None,
            started_at: Some(format!("{seq}Z")),
            ended_at: None,
            timing: None,
            raw: None,
        };

        maybe_emit_permission_intervention(
            &bus,
            "project-1",
            None,
            &context,
            &permission_event("permission-1", 1),
        );
        maybe_emit_permission_intervention(
            &bus,
            "project-1",
            None,
            &context,
            &permission_event("permission-2", 2),
        );

        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, RuntimeInterventionKind::PermissionRequested);
        assert_eq!(
            events[0].1,
            "project-1:run-001:round-001:plan:attempt-001:permission-requested:permission-1"
        );
        assert_eq!(
            events[1].1,
            "project-1:run-001:round-001:plan:attempt-001:permission-requested:permission-2"
        );
    }

    #[test]
    fn maybe_emit_elicitation_intervention_ignores_non_pending_events() {
        let bus = gold_band::app::observability::RuntimeLifecycleBus::new();
        let seen = Arc::new(Mutex::new(0usize));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if matches!(event, RuntimeLifecycleEvent::InterventionRequested { .. }) {
                *seen_for_handler.lock().unwrap() += 1;
            }
        }));

        maybe_emit_elicitation_intervention(
            &bus,
            "project-1",
            None,
            &gold_band::app::AcpLiveEventContext {
                task_id: "task-001".to_string(),
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                node_id: "plan".to_string(),
                attempt_id: "attempt-001".to_string(),
                outer_node_id: None,
                outer_attempt_id: None,
            },
            &AcpUiEvent {
                kind: "elicitationRequest".to_string(),
                id: "elicit-001".to_string(),
                seq: 1,
                timestamp: "1Z".to_string(),
                session_id: None,
                status: Some("completed".to_string()),
                title: None,
                content: None,
                tool_call_id: None,
                started_seq: None,
                ended_seq: None,
                started_at: Some("1Z".to_string()),
                ended_at: Some("2Z".to_string()),
                timing: None,
                raw: None,
            },
        );

        assert_eq!(*seen.lock().unwrap(), 0);
    }
}
