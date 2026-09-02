use gold_band::acp::client;
use gold_band::acp::commands::{AcpCommandCatalog, parse_available_commands};
use gold_band::acp::elicitation::{ElicitationAction, write_elicitation_response};
use gold_band::acp::events::{
    AcpTurnExecutionClaim, AcpUiEvent, compact_live_conversation_event, current_timestamp,
    load_session_metadata,
};
use gold_band::acp::permission::{PendingPermissionState, write_permission_response_if_pending};
use gold_band::acp::prompt_queue::{
    AUTO_DISPATCH_USER_PRIORITY_GRACE_MS, AutoClaimResult, PromptQueueError,
    TerminalDispatchRecovery, auto_dispatch_is_suspended, claim_next_for_auto_dispatch,
    claim_queued_prompt, clear_auto_dispatch_reply_batch, clear_auto_dispatch_suspension,
    complete_accepted_prompt, delete_queued_prompt, enqueue_prompt, load_prompt_queue,
    mark_user_priority, record_auto_dispatch_reply_completion, recover_terminal_dispatch,
    release_queued_prompt, reorder_queued_prompts, request_auto_dispatch_suspension,
    settle_dispatching_prompt, suspend_auto_dispatch, take_queued_prompt,
};
use gold_band::acp::turn_files::{
    ATTACHMENT_ACCESS_DENIED, ATTACHMENT_NOT_FOUND, CHANGE_SET_NOT_FOUND, TurnFileChangeSet,
    TurnFileStore, VERSION_NOT_FOUND,
};
use gold_band::app::{
    AcpPromptLifecycleEvent, AcpTurnBatchProgress, AcpTurnOutcome, App, AutoTemplate,
    AutoTemplateStore, CreateTaskInput, ImportProfilesInput, ImportProfilesResult,
    ProfileCommandError, ProfileEntry, ProfileInput, ProfileList, RuntimeInterventionKind,
    RuntimeLifecycleEvent, WorkflowTemplateStore,
};
use gold_band::domain::{
    NodeOutcome, PauseReason, RunOutcome, RunStatus, SessionMode, TurnControlMode,
};
use gold_band::dsl::{AiDynamicAgentStrategy, NodeDsl, WorkflowDsl, WorkflowValidationError};
use gold_band::dynamic::{DynamicNodeStatus, DynamicRunStatus};
use gold_band::dynamic_store::load_dynamic_graph;
use gold_band::runtime::{NodeState, RunState, WorkerRefState};
use gold_band::scheduler::db::ScheduledTaskDatabase;
use gold_band::skill::SkillCommandError;
use gold_band::storage::read_json;
use gold_band::storage::sqlite::{self, AttemptIndexContext};
use std::path::{Component, Path, PathBuf};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use camino::Utf8PathBuf;
use gold_band::config::{
    AcpAdapterConfig, AppearancePreference, AvatarPreference, AvatarShapePreference,
    ConversationAutoConfig, DEFAULT_CUSTOM_AGENT_ICON, DesktopLanguage, FontSizePreference,
    FontStackPreference, MAX_DESKTOP_WALLPAPER_OPACITY_PERCENT, MAX_FONT_FAMILY_CHARS,
    MAX_FONT_STACK_FAMILIES, MIN_DESKTOP_WALLPAPER_OPACITY_PERCENT, ManagedAgentConfig,
    ManagedAgentId, PersonalizationAvatarShape, PersonalizationPreference,
    WallpaperImagePreference, normalize_desktop_editor_font_size, normalize_desktop_ui_font_size,
};
use gold_band::observability::set_runtime_log_level;
use gold_band::provider::{
    AcpLiveTimelinePosition, ConversationPromptInput, MAX_USER_PROMPT_QUOTE_CHARS,
    MAX_USER_PROMPT_QUOTE_ID_BYTES, MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES, MAX_USER_PROMPT_QUOTES,
    UserPromptQuote, conversation_prompt_text, select_config_options_from_capabilities,
    supported_models_from_capabilities, supported_modes_from_capabilities,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tracing::{info, warn};
use uuid::Uuid;

use crate::avatar::{
    AvatarKind, AvatarPreferencesVm, AvatarShape, SaveDesktopAvatarInput, clear_avatar,
    load_resolved_avatar_preferences, save_avatar_image, save_avatar_shape, select_recent_avatar,
};
use crate::conversation_attention::{
    ConversationTerminalResultKind, ConversationTerminalResultVm, record_terminal_result,
};
use crate::conversation_workspace::{
    RuntimeWorkspaceAccessError, app_for_workspace, validate_runtime_workspace_access,
    workspace_entry_for_project,
};
use crate::i18n::Translator;
use crate::metrics::{MetricsSettingsVm, metrics_settings, normalize_metrics_base_url};
use crate::state::{
    DesktopState, NotificationAttentionInput, RecoveredConversationRun, UpdateBadgeSeenTarget,
};
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
    conversation_is_orchestrated, conversation_run_mode, conversation_session_successor,
    conversation_task_activity_from_prompt,
};
use crate::wallpaper::{
    ImportDesktopWallpaperInput, RestoreThemeDesktopWallpaperInput,
    SaveDesktopWallpaperOpacityInput, SelectRecentDesktopWallpaperInput, import_wallpaper_image,
    load_resolved_wallpaper_preferences, reconcile_wallpaper_personalization,
    select_recent_wallpaper,
};

const ACP_SESSION_EVENT: &str = "gold-band://acp-session-updated";
const AGENT_REGISTRY_UPDATED_EVENT: &str = "gold-band://agent-registry-updated";
const AGENT_COMMANDS_UPDATED_EVENT: &str = "gold-band://agent-commands-updated";
const CONVERSATION_RUN_STATE_EVENT: &str = "gold-band://conversation-run-state-updated";
const CONVERSATION_TERMINAL_RESULT_EVENT: &str = "gold-band://conversation-terminal-result-updated";
const PERMISSION_REQUESTED_DEDUP_SUFFIX: &str = "permission-requested";
const ELICITATION_REQUESTED_DEDUP_SUFFIX: &str = "elicitation-requested";
const QUEUED_PROMPT_ID_PREFIX: &str = "turn-queued-";

pub type CommandResult<T> = Result<T, CommandErrorVm>;

fn existing_attempt_prompt_session_target(
    worker_ref: Option<WorkerRefState>,
) -> (SessionMode, Option<serde_json::Value>) {
    (
        SessionMode::Continue,
        worker_ref.and_then(|worker_ref| worker_ref.continue_ref),
    )
}

pub(crate) async fn spawn_blocking_command<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::handle()
        .inner()
        .spawn_blocking(operation)
        .await
        .map_err(|error| {
            let kind = if error.is_panic() {
                "panic"
            } else if error.is_cancelled() {
                "cancelled"
            } else {
                "unknown"
            };
            let detail = if error.is_panic() {
                let payload = error.into_panic();
                payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".to_string())
            } else {
                error.to_string()
            };
            warn!(join_kind = kind, %detail, "blocking command task failed to join");
            CommandErrorVm::new(
                "app.task-join-failed",
                serde_json::json!({
                    "kind": kind,
                    "detail": detail,
                }),
            )
        })?
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
    let dynamic_graph_path = app.paths.dynamic_graph_file(
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        outer_node_id,
        outer_attempt_id,
    );
    let dynamic_graph =
        load_dynamic_graph(&dynamic_graph_path, &app.paths.repo_root).map_err(command_error)?;
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
    if let Some(target) = conversation_session_successor(
        app,
        &locator.task_id,
        &locator.run_id,
        &locator.round_id,
        &locator.node_id,
        &locator.attempt_id,
        locator.outer_node_id(),
        locator.outer_attempt_id(),
    )
    .map_err(command_error)?
    {
        return Err(CommandErrorVm::new(
            "conversation.session-superseded",
            serde_json::json!({
                "roundId": target.round_id,
                "nodeId": target.node_id,
                "attemptId": target.attempt_id,
                "outerNodeId": target.outer_node_id,
                "outerAttemptId": target.outer_attempt_id,
            }),
        ));
    }
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

fn acp_lifecycle_path(attempt_dir: &camino::Utf8Path) -> Utf8PathBuf {
    // New lifecycle writes always converge on the runtime snapshot. Admission
    // seeds it from a legacy session file when needed, avoiding a turn whose
    // stop and terminal transitions are split across two metadata files.
    attempt_dir.join("acp.snapshot.json")
}

fn prompt_submission_admission_error(error: anyhow::Error) -> CommandErrorVm {
    if error.to_string().starts_with("acp.prompt-session-busy") {
        CommandErrorVm::new("conversation.prompt-session-busy", serde_json::json!({}))
    } else if error
        .to_string()
        .starts_with("acp.prompt-submission-conflict")
    {
        CommandErrorVm::new(
            "conversation.prompt-submission-conflict",
            serde_json::json!({}),
        )
    } else {
        command_error(error)
    }
}

fn settle_failed_prompt_submission(
    app: &App,
    locator: &AttemptLocator,
    turn_id: &str,
    operation_id: Option<&str>,
    expected_revision: u64,
) -> bool {
    let lifecycle_path = acp_lifecycle_path(&locator.attempt_dir(app));
    let decided_at = gold_band::acp::events::current_timestamp();
    match gold_band::acp::events::persist_session_turn_terminal_owned(
        &lifecycle_path,
        turn_id,
        operation_id,
        expected_revision,
        gold_band::acp::events::AcpLatestTurnStatus::Failed,
        "provider-error",
        &decided_at,
    ) {
        Ok(Some(header)) => {
            let failed =
                header.latest_turn_status == gold_band::acp::events::AcpLatestTurnStatus::Failed;
            if failed {
                touch_terminal_task_activity_best_effort(
                    app,
                    locator,
                    Some(turn_id),
                    "prompt-turn-failed",
                );
            }
            failed
        }
        Ok(None) => {
            tracing::debug!(%turn_id, "skipping stale ACP prompt terminal notification");
            false
        }
        Err(error) => {
            warn!(%error, %turn_id, "failed to settle rejected background ACP prompt");
            false
        }
    }
}

fn conversation_prompt_submission(
    app: &App,
    locator: &AttemptLocator,
    turn_id: String,
    input: &ConversationPromptInput,
    attachment_paths: &[String],
) -> gold_band::acp::events::AcpPromptSubmission {
    let attempt_dir = locator.attempt_dir(app);
    gold_band::acp::events::AcpPromptSubmission {
        turn_id,
        operation_id: format!("prompt:{}", uuid::Uuid::new_v4().simple()),
        adapter_id: acp_turn_provider_id(app, locator).unwrap_or_else(|| "unknown".to_string()),
        adapter_display_name: acp_turn_agent_label(app, locator),
        cwd: attempt_dir.to_string(),
        input: input.clone(),
        attachment_paths: attachment_paths.to_vec(),
        admitted_at: gold_band::acp::events::current_timestamp(),
    }
}

fn existing_conversation_prompt_turn(
    app: &App,
    locator: &AttemptLocator,
    prompt_id: Option<&str>,
    input: &ConversationPromptInput,
    attachment_paths: &[String],
) -> CommandResult<Option<gold_band::acp::events::AcpTurnAdmission>> {
    let Some(turn_id) = prompt_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let submission =
        conversation_prompt_submission(app, locator, turn_id.to_string(), input, attachment_paths);
    gold_band::acp::events::inspect_session_turn(
        &acp_lifecycle_path(&locator.attempt_dir(app)),
        &submission,
    )
    .map_err(prompt_submission_admission_error)
}

fn admit_conversation_prompt_turn(
    app_handle: &AppHandle,
    app: &App,
    project_id: Option<String>,
    locator: &AttemptLocator,
    prompt_id: Option<String>,
    input: &ConversationPromptInput,
    attachment_paths: &[String],
) -> CommandResult<gold_band::acp::events::AcpTurnAdmission> {
    let turn_id = prompt_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("turn-{}", uuid::Uuid::new_v4().simple()));
    let attempt_dir = locator.attempt_dir(app);
    let agent_label = acp_turn_agent_label(app, locator);
    let submission =
        conversation_prompt_submission(app, locator, turn_id.clone(), input, attachment_paths);
    let admitted_at = submission.admitted_at.clone();
    let lifecycle_path = acp_lifecycle_path(&attempt_dir);
    if let Some(existing) =
        gold_band::acp::events::inspect_session_turn(&lifecycle_path, &submission)
            .map_err(prompt_submission_admission_error)?
    {
        return Ok(existing);
    }
    let preflight = ensure_conversation_prompt_available(app, locator);
    finish_acp_prompt_preflight(app, locator, &turn_id, &agent_label, preflight)?;
    let admission = gold_band::acp::events::begin_session_turn(&lifecycle_path, &submission)
        .map_err(prompt_submission_admission_error)?;
    if admission.started() {
        touch_task_activity_at_best_effort(
            app,
            &locator.task_id,
            &admitted_at,
            "user-prompt-admitted",
        );
        emit_direct_turn_started(app, locator);
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
    }
    let header = admission.header();
    info!(
        project_id = %app.paths.project_id,
        task_id = %locator.task_id,
        run_id = %locator.run_id,
        round_id = %locator.round_id,
        node_id = %locator.node_id,
        attempt_id = %locator.attempt_id,
        outer_node_id = ?locator.outer_node_id,
        outer_attempt_id = ?locator.outer_attempt_id,
        turn_id = ?header.turn_id,
        operation_id = ?header.operation_id,
        revision = header.revision,
        admission = if admission.started() { "started" } else { "existing" },
        "conversation prompt admitted"
    );
    Ok(admission)
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

fn acp_turn_outcome_label(outcome: AcpTurnOutcome) -> &'static str {
    match outcome {
        AcpTurnOutcome::Completed => "completed",
        AcpTurnOutcome::Failed => "failed",
        AcpTurnOutcome::Cancelled => "cancelled",
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

// ── Direct metrics background worker ──────────────────────────────────
// The command thread must never block on file I/O or mutex operations.
// These lightweight jobs carry only String data; the worker thread does
// all heavy lifting (task_show, observability snapshot, ACP session read).

#[derive(Debug, Clone)]
enum DirectMetricsJob {
    TurnStarted {
        locator: AttemptLocator,
        repo_root: String,
    },
    TurnFinished {
        locator: AttemptLocator,
        turn_id: String,
        agent_label: String,
        outcome: AcpTurnOutcome,
        repo_root: String,
    },
    InterventionRequested {
        context: gold_band::app::AcpLiveEventContext,
        request_id: String,
        kind: RuntimeInterventionKind,
        repo_root: String,
    },
}

const DIRECT_METRICS_QUEUE_CAPACITY: usize = 512;

static DIRECT_METRICS_SENDER: std::sync::OnceLock<std::sync::mpsc::SyncSender<DirectMetricsJob>> =
    std::sync::OnceLock::new();

fn direct_metrics_sender() -> Option<std::sync::mpsc::SyncSender<DirectMetricsJob>> {
    DIRECT_METRICS_SENDER.get().cloned()
}

fn init_direct_metrics_worker(app: App) {
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<DirectMetricsJob>(DIRECT_METRICS_QUEUE_CAPACITY);
    if DIRECT_METRICS_SENDER.set(sender).is_err() {
        return; // already initialised
    }

    let _ = std::thread::Builder::new()
        .name("direct-metrics-worker".into())
        .spawn(move || {
            while let Ok(job) = receiver.recv() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut scoped_app = app.clone_for_background();
                    match &job {
                        DirectMetricsJob::TurnStarted { locator, repo_root } => {
                            scoped_app.paths = gold_band::storage::GoldBandPaths::new(
                                camino::Utf8PathBuf::from(repo_root),
                            );
                            build_direct_turn_metrics_fact(&scoped_app, locator, None);
                        }
                        DirectMetricsJob::TurnFinished {
                            locator,
                            outcome,
                            repo_root,
                            ..
                        } => {
                            scoped_app.paths = gold_band::storage::GoldBandPaths::new(
                                camino::Utf8PathBuf::from(repo_root),
                            );
                            build_direct_turn_metrics_fact(&scoped_app, locator, Some(*outcome));
                        }
                        DirectMetricsJob::InterventionRequested {
                            context,
                            request_id,
                            kind,
                            repo_root,
                        } => {
                            scoped_app.paths = gold_band::storage::GoldBandPaths::new(
                                camino::Utf8PathBuf::from(repo_root),
                            );
                            build_request_intervention_metrics(
                                &scoped_app,
                                context,
                                request_id,
                                *kind,
                            );
                        }
                    }
                }));
            }
        });
}

fn emit_acp_turn_finished(
    app: &App,
    locator: &AttemptLocator,
    turn_id: &str,
    agent_label: &str,
    outcome: AcpTurnOutcome,
    batch_progress: AcpTurnBatchProgress,
) {
    info!(
        project_id = %app.paths.project_id,
        task_id = %locator.task_id,
        run_id = %locator.run_id,
        round_id = %locator.round_id,
        node_id = %locator.node_id,
        attempt_id = %locator.attempt_id,
        outer_node_id = ?locator.outer_node_id,
        outer_attempt_id = ?locator.outer_attempt_id,
        %turn_id,
        outcome = acp_turn_outcome_label(outcome),
        completed_reply_count = batch_progress.completed_reply_count,
        batch_continues = batch_progress.continues,
        "conversation ACP turn reached terminal state"
    );
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
    if let Some(sender) = direct_metrics_sender() {
        let _ = sender.try_send(DirectMetricsJob::TurnFinished {
            locator: locator.clone(),
            turn_id: turn_id.to_string(),
            agent_label: agent_label.to_string(),
            outcome,
            repo_root: app.paths.repo_root.to_string(),
        });
    }
}

fn emit_direct_turn_started(app: &App, locator: &AttemptLocator) {
    if let Some(sender) = direct_metrics_sender() {
        let _ = sender.try_send(DirectMetricsJob::TurnStarted {
            locator: locator.clone(),
            repo_root: app.paths.repo_root.to_string(),
        });
    }
}

fn build_direct_turn_metrics_fact(
    app: &App,
    locator: &AttemptLocator,
    outcome: Option<AcpTurnOutcome>,
) {
    if !app.metrics_collection_enabled() {
        return;
    }
    if gold_band::app::direct_conversation_agent_label(app, &locator.task_id).is_none() {
        return;
    }
    let Ok(task) = app.task_show(&locator.task_id) else {
        return;
    };
    let Some(task_uuid) = task.uuid else { return };
    let attempt_dir = locator.attempt_dir(app);
    let occurred_at = current_timestamp();
    let execution_id = task_uuid.clone();
    let attempt_key = format!("direct:{task_uuid}");
    let attempt_path = app
        .paths
        .run_dir(&locator.task_id, &locator.run_id)
        .join("observability")
        .join(&execution_id)
        .join(&execution_id)
        .join(gold_band::app::observability::OBSERVABILITY_SNAPSHOT_FILE);
    let is_follow_up = if outcome.is_none() {
        app.direct_metrics_is_follow_up(&attempt_key, Some(attempt_dir.as_path()), &attempt_path)
    } else {
        false
    };
    let active_turn = if outcome.is_none() {
        match app.active_metrics_turn(&attempt_key) {
            Some(turn) => turn,
            None => {
                let usage_baseline =
                    gold_band::app::App::direct_usage_baseline(Some(attempt_dir.as_path()));
                let turn = gold_band::app::ActiveMetricTurn::new(
                    execution_id.clone(),
                    execution_id.clone(),
                    1,
                    usage_baseline,
                );
                app.begin_metrics_turn(attempt_key.clone(), turn.clone());
                turn
            }
        }
    } else {
        let Some(turn) = app.active_metrics_turn(&attempt_key) else {
            return;
        };
        turn
    };
    let provider = acp_turn_provider_id(app, locator);
    let model = current_acp_session_model_name(&attempt_dir);
    let attempt_state =
        app.update_observability_state(&active_turn.attempt_id, attempt_path, |state| {
            if outcome.is_none() {
                state.record_started_at(occurred_at.clone());
                if is_follow_up {
                    state.record_follow_up();
                }
            }
            if outcome.is_some() {
                let segments = gold_band::app::App::direct_usage_segments_after(
                    Some(attempt_dir.as_path()),
                    active_turn.usage_baseline_turn_seq,
                );
                let usages = gold_band::app::App::direct_model_usages_from_segments(
                    &segments,
                    provider.as_deref(),
                    model.as_deref(),
                );
                for usage in usages {
                    state.record_model_usage(usage);
                }
                if segments.is_empty()
                    && let (Some(provider), Some(model)) = (provider.as_ref(), model.as_ref())
                {
                    let usage = gold_band::acp::events::read_attempt_metrics(
                        &attempt_dir.join("acp.session.json"),
                    );
                    state.record_cumulative_model_usage(
                        provider.clone(),
                        model.clone(),
                        gold_band::app::observability::TokenUsage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            cache_read_tokens: usage.cache_read_tokens,
                            total_tokens: usage.total_tokens,
                        },
                        usage.elapsed_ms,
                    );
                }
            }
            state.next_revision();
        });
    let direct_revision = attempt_state.event_revision;
    let event_type = if outcome.is_some() {
        gold_band::app::observability::LifecycleEventType::ExecutionCompleted
    } else {
        gold_band::app::observability::LifecycleEventType::ExecutionStarted
    };
    let mut fact = gold_band::app::observability::MetricsLifecycleFact::new(
        event_type,
        direct_revision,
        occurred_at.clone(),
        crate::metrics::get_system_username(),
        app.paths.repo_root.to_string(),
        gold_band::app::observability::MetricsSessionMode::Direct,
        task_uuid,
        gold_band::app::observability::ExecutionKind::Turn,
        active_turn.execution_id.clone(),
    );
    fact.task_title = task.title.clone();
    fact.attempt_id = Some(active_turn.attempt_id.clone());
    fact.attempt_index = Some(active_turn.attempt_index);
    fact.provider = provider;
    fact.model = model;
    fact.collection_state_recovered = attempt_state.collection_state_recovered;
    if let Some(outcome) = outcome {
        fact.outcome = Some(match outcome {
            AcpTurnOutcome::Completed => gold_band::app::observability::ExecutionOutcome::Completed,
            AcpTurnOutcome::Failed => gold_band::app::observability::ExecutionOutcome::Failed,
            AcpTurnOutcome::Cancelled => gold_band::app::observability::ExecutionOutcome::Cancelled,
        });
        fact.terminal_reason = Some(match outcome {
            AcpTurnOutcome::Completed => gold_band::app::observability::TerminalReason::Completed,
            AcpTurnOutcome::Failed => gold_band::app::observability::TerminalReason::ProviderError,
            AcpTurnOutcome::Cancelled => {
                gold_band::app::observability::TerminalReason::UserCancelled
            }
        });
        let usages = attempt_state.model_usages();
        let elapsed_sum = usages
            .iter()
            .filter_map(|usage| usage.acp_session_elapsed_ms)
            .fold(None, |total, value| {
                Some(total.unwrap_or(0u64).saturating_add(value))
            });
        let sum = |get: fn(&gold_band::app::observability::TokenUsage) -> Option<u64>| {
            usages
                .iter()
                .filter_map(|usage| get(&usage.usage))
                .fold(None, |total, value| {
                    Some(total.unwrap_or(0u64).saturating_add(value))
                })
        };
        if !usages.is_empty() {
            fact.usage = Some(gold_band::app::observability::TokenUsage {
                input_tokens: sum(|u| u.input_tokens),
                output_tokens: sum(|u| u.output_tokens),
                cache_read_tokens: sum(|u| u.cache_read_tokens),
                total_tokens: sum(|u| u.total_tokens),
            });
            fact.model_usages = Some(usages);
        }
        fact.timing = Some(gold_band::app::observability::LifecycleTiming {
            started_at: attempt_state
                .started_at
                .clone()
                .unwrap_or_else(|| occurred_at.clone()),
            ended_at: Some(occurred_at),
            acp_session_elapsed_ms: elapsed_sum,
        });
        fact.counters = Some(attempt_state.counters.clone());
    }
    app.emit_lifecycle_event(RuntimeLifecycleEvent::MetricsFact(fact));
    if outcome.is_some() {
        app.release_observability_state(&active_turn.execution_id);
        app.end_metrics_turn(&attempt_key);
    }
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
    timeline_generation: Option<u64>,
    timeline_revision: Option<u64>,
    project_id: Option<String>,
    task_id: String,
    task_uuid: Option<String>,
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
    task_activity_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRunStateUpdatedEventVm {
    event_kind: String,
    project_id: String,
    task_id: String,
    task_uuid: Option<String>,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    status: RunStatus,
    outcome: Option<RunOutcome>,
}

pub(crate) fn emit_recovered_conversation_run_state(
    app_handle: &AppHandle,
    recovered: &RecoveredConversationRun,
) {
    let _ = app_handle.emit(
        CONVERSATION_RUN_STATE_EVENT,
        ConversationRunStateUpdatedEventVm {
            event_kind: "run-recovered".to_string(),
            project_id: recovered.project_id.clone(),
            task_id: recovered.task_id.clone(),
            task_uuid: recovered.task_uuid.clone(),
            run_id: recovered.run_id.clone(),
            round_id: recovered.round_id.clone(),
            node_id: recovered.node_id.clone(),
            attempt_id: recovered.attempt_id.clone(),
            status: recovered.status,
            outcome: recovered.outcome,
        },
    );
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

async fn resolve_runtime_command_app(
    state: &DesktopState,
    project_id: Option<&str>,
) -> Result<App, CommandErrorVm> {
    let app = resolve_command_app(state, project_id)?;
    validate_runtime_workspace_for_command(&app.paths.project_id, app.paths.repo_root.as_str())
        .await?;
    Ok(app)
}

pub(crate) fn register_lifecycle_subscribers(app: &App, app_handle: &AppHandle) {
    if crate::channel::current_channel_config().channel == "wb"
        && crate::metrics::metrics_settings(&app.config).enabled
        && crate::metrics::get_api_key(&app.config).is_some()
    {
        app.lifecycle_bus.subscribe_named_with_mode(
            "core.metrics-producer",
            gold_band::app::observability::SubscriberMode::Inline,
            app.create_metrics_fact_producer(),
        );
        app.lifecycle_bus.subscribe_named(
            "desktop.metrics",
            crate::metrics::create_metrics_subscriber(app_handle.clone()),
        );
        init_direct_metrics_worker(app.clone_for_background());
    }
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
    app.lifecycle_bus.subscribe_named(
        "desktop.conversation-terminal-result",
        create_conversation_terminal_result_subscriber(app_handle.clone()),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConversationTerminalResultCandidate {
    project_id: String,
    task_id: String,
    result: ConversationTerminalResultVm,
    requires_direct_mode_check: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationTerminalResultUpdatedEventVm {
    project_id: String,
    task_id: String,
    unread_terminal_result: ConversationTerminalResultVm,
}

fn conversation_terminal_result_candidate(
    event: &RuntimeLifecycleEvent,
) -> Option<ConversationTerminalResultCandidate> {
    match event {
        RuntimeLifecycleEvent::RunCompleted {
            event_id,
            occurred_at,
            project_id,
            task_id,
            run_id,
            outcome,
            completion_agent_label,
            ..
        } if completion_agent_label.is_some() && *outcome != RunOutcome::Success => {
            Some(ConversationTerminalResultCandidate {
                project_id: project_id.clone(),
                task_id: task_id.clone(),
                result: ConversationTerminalResultVm {
                    event_id: event_id.clone(),
                    run_id: run_id.clone(),
                    kind: match outcome {
                        RunOutcome::Killed => ConversationTerminalResultKind::Stopped,
                        RunOutcome::Failure => ConversationTerminalResultKind::Failed,
                        RunOutcome::Success => return None,
                    },
                    occurred_at: occurred_at.clone(),
                },
                requires_direct_mode_check: false,
            })
        }
        RuntimeLifecycleEvent::AcpTurnFinished {
            event_id,
            occurred_at,
            project_id,
            task_id,
            run_id,
            outcome,
            batch_progress,
            ..
        } if !batch_progress.continues => Some(ConversationTerminalResultCandidate {
            project_id: project_id.clone(),
            task_id: task_id.clone(),
            result: ConversationTerminalResultVm {
                event_id: event_id.clone(),
                run_id: run_id.clone(),
                kind: match outcome {
                    AcpTurnOutcome::Completed => ConversationTerminalResultKind::Completed,
                    AcpTurnOutcome::Cancelled => ConversationTerminalResultKind::Stopped,
                    AcpTurnOutcome::Failed => ConversationTerminalResultKind::Failed,
                },
                occurred_at: occurred_at.clone(),
            },
            requires_direct_mode_check: true,
        }),
        _ => None,
    }
}

fn create_conversation_terminal_result_subscriber(
    app_handle: AppHandle,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
    Arc::new(move |event| {
        let Some(candidate) = conversation_terminal_result_candidate(&event) else {
            return;
        };
        let Some(desktop_state) = app_handle.try_state::<DesktopState>() else {
            warn!("desktop state unavailable while recording Direct terminal result");
            return;
        };
        let app = match resolve_command_app(&desktop_state, Some(&candidate.project_id)) {
            Ok(app) => app,
            Err(error) => {
                warn!(
                    code = %error.code,
                    project_id = %candidate.project_id,
                    task_id = %candidate.task_id,
                    "Direct terminal result workspace resolution failed"
                );
                return;
            }
        };
        if candidate.requires_direct_mode_check
            && conversation_run_mode(&app, &candidate.task_id)
                != Some(gold_band::config::ConversationRunMode::Direct)
        {
            return;
        }
        let write_guard = match desktop_state.conversation_attention_write_guard() {
            Ok(guard) => guard,
            Err(error) => {
                warn!(?error, "Direct terminal result write lock failed");
                return;
            }
        };
        if app.task_show(&candidate.task_id).is_err() {
            return;
        }
        let recorded = match record_terminal_result(&app, &candidate.task_id, candidate.result) {
            Ok(recorded) => recorded,
            Err(error) => {
                warn!(
                    ?error,
                    project_id = %candidate.project_id,
                    task_id = %candidate.task_id,
                    "Direct terminal result persistence failed"
                );
                return;
            }
        };
        drop(write_guard);
        if !recorded.changed {
            return;
        }
        if let Some(unread_terminal_result) = recorded.unread_terminal_result {
            let _ = app_handle.emit(
                CONVERSATION_TERMINAL_RESULT_EVENT,
                ConversationTerminalResultUpdatedEventVm {
                    project_id: candidate.project_id,
                    task_id: candidate.task_id,
                    unread_terminal_result,
                },
            );
        }
    })
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
        RuntimeLifecycleEvent::NodeStarted {
            project_id,
            task_id,
            task_uuid,
            run_id,
            round_id,
            node_id,
            attempt_id,
            metrics_unit_kind: None,
            ..
        } => Some(ConversationRunStateUpdatedEventVm {
            event_kind: "node-started".to_string(),
            project_id,
            task_id,
            task_uuid,
            run_id,
            round_id,
            node_id,
            attempt_id,
            status: RunStatus::Running,
            outcome: None,
        }),
        RuntimeLifecycleEvent::RunPaused {
            project_id,
            task_id,
            task_uuid,
            run_id,
            round_id,
            node_id,
            attempt_id,
            ..
        } => Some(ConversationRunStateUpdatedEventVm {
            event_kind: "run-paused".to_string(),
            project_id,
            task_id,
            task_uuid,
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
            task_uuid,
            run_id,
            round_id,
            node_id,
            attempt_id,
            outcome,
            ..
        } => Some(ConversationRunStateUpdatedEventVm {
            event_kind: "run-completed".to_string(),
            project_id,
            task_id,
            task_uuid,
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
) -> Arc<
    dyn Fn(
            gold_band::app::AcpLiveEventContext,
            AcpUiEvent,
            AcpLiveTimelinePosition,
        ) -> anyhow::Result<()>
        + Send
        + Sync,
> {
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
) -> Result<ConfiguredConversationApp, CommandErrorVm> {
    let app = resolve_command_app(state, project_id)?;
    let pid = project_id.map(|s| s.to_string());
    Ok(configure_conversation_runtime_callbacks(
        app,
        app_handle.clone(),
        pid,
    ))
}

async fn resolve_runtime_command_app_with_emitters(
    app_handle: &AppHandle,
    state: &DesktopState,
    project_id: Option<&str>,
) -> Result<ConfiguredConversationApp, CommandErrorVm> {
    let app = resolve_runtime_command_app(state, project_id).await?;
    let pid = project_id.map(str::to_string);
    Ok(configure_conversation_runtime_callbacks(
        app,
        app_handle.clone(),
        pid,
    ))
}

pub(crate) struct ConfiguredConversationApp(App);

impl std::ops::Deref for ConfiguredConversationApp {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ConfiguredConversationApp {
    fn into_inner(self) -> App {
        self.0
    }
}

pub(crate) fn configure_conversation_runtime_callbacks(
    app: App,
    app_handle: AppHandle,
    project_id: Option<String>,
) -> ConfiguredConversationApp {
    let bg_app = app.clone_for_background();
    let live_update = acp_live_update_emitter_for_app(&app, app_handle.clone(), project_id.clone());
    let prompt_turn_lifecycle = prompt_turn_lifecycle_callback(
        app_handle.clone(),
        app.clone_for_background(),
        project_id.clone(),
    );
    ConfiguredConversationApp(
        app.with_acp_live_update(live_update)
            .with_acp_session_update(acp_session_update_emitter(app_handle, bg_app, project_id))
            .with_prompt_turn_lifecycle(prompt_turn_lifecycle),
    )
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
        let terminal = matches!(&event, AcpPromptLifecycleEvent::Finished { .. });
        let locator = AttemptLocator::new(
            context.task_id,
            context.run_id,
            context.round_id,
            context.node_id,
            context.attempt_id,
            context.outer_node_id,
            context.outer_attempt_id,
        );
        let result = process_prompt_turn_lifecycle(
            &app,
            locator.clone(),
            event,
            |locator, successful, completion| {
                schedule_direct_prompt_queue_drain(
                    app_handle.clone(),
                    project_id.clone(),
                    direct_prompt_queue_drain_app(&app),
                    locator,
                    successful,
                    completion,
                );
            },
        );
        if result.is_ok() && terminal {
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
        }
        result
    })
}

fn touch_task_activity_at_best_effort(
    app: &App,
    task_id: &str,
    activity_at: &str,
    reason: &'static str,
) {
    if let Err(error) =
        crate::view_models_conversation::touch_conversation_activity_at(app, task_id, activity_at)
    {
        warn!(
            project_id = %app.paths.project_id,
            %task_id,
            %reason,
            %error,
            "failed to project durable Task conversation activity"
        );
    }
}

fn touch_terminal_task_activity_best_effort(
    app: &App,
    locator: &AttemptLocator,
    expected_turn_id: Option<&str>,
    reason: &'static str,
) {
    let result = (|| -> anyhow::Result<()> {
        let lifecycle_path = acp_lifecycle_path(&locator.attempt_dir(app));
        let metadata = gold_band::acp::events::read_session_metadata_value(&lifecycle_path, None)?;
        if let Some(expected_turn_id) = expected_turn_id
            && metadata.get("turnId").and_then(serde_json::Value::as_str) != Some(expected_turn_id)
        {
            tracing::debug!(
                project_id = %app.paths.project_id,
                task_id = %locator.task_id,
                run_id = %locator.run_id,
                %expected_turn_id,
                "skipping stale terminal Task activity observation"
            );
            return Ok(());
        }
        anyhow::ensure!(
            metadata
                .get("liveTurnActivity")
                .and_then(serde_json::Value::as_str)
                == Some("idle")
                && metadata
                    .get("latestTurnStatus")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|status| status != "none"),
            "canonical prompt turn has not reached terminal state"
        );
        let activity_at = metadata
            .get("updatedAt")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("canonical terminal activity timestamp is missing"))?;
        crate::view_models_conversation::touch_conversation_activity_at(
            app,
            &locator.task_id,
            activity_at,
        )
    })();
    if let Err(error) = result {
        warn!(
            project_id = %app.paths.project_id,
            task_id = %locator.task_id,
            run_id = %locator.run_id,
            %reason,
            %error,
            "failed to project canonical terminal Task conversation activity"
        );
    }
}

fn process_prompt_turn_lifecycle(
    app: &App,
    locator: AttemptLocator,
    event: AcpPromptLifecycleEvent,
    mut schedule_finished: impl FnMut(AttemptLocator, bool, Option<DeferredTurnCompletion>),
) -> anyhow::Result<()> {
    match event {
        AcpPromptLifecycleEvent::Accepted { prompt_id } => {
            complete_accepted_prompt(&locator.attempt_dir(app), &prompt_id)?;
        }
        AcpPromptLifecycleEvent::Finished {
            prompt_id,
            successful,
        } => {
            touch_terminal_task_activity_best_effort(
                app,
                &locator,
                prompt_id.as_deref(),
                "prompt-turn-finished",
            );
            let completion = prompt_id.map(|turn_id| DeferredTurnCompletion {
                turn_id,
                agent_label: acp_turn_agent_label(app, &locator),
            });
            schedule_finished(locator, successful, completion);
        }
    }
    Ok(())
}

fn direct_prompt_queue_drain_app(app: &App) -> App {
    app.clone_for_background()
}

fn queued_user_turn_app(app: &App) -> App {
    app.clone_for_background().without_scheduled_turn_context()
}

fn schedule_direct_prompt_queue_drain(
    app_handle: AppHandle,
    project_id: Option<String>,
    app: App,
    locator: AttemptLocator,
    successful: bool,
    completed_turn: Option<DeferredTurnCompletion>,
) {
    if conversation_run_mode(&app, &locator.task_id)
        != Some(gold_band::config::ConversationRunMode::Direct)
    {
        return;
    }
    let attempt_dir = locator.attempt_dir(&app);
    if let Some(completion) = completed_turn.as_ref() {
        if let Err(error) = settle_dispatching_prompt(&attempt_dir, &completion.turn_id) {
            warn!(
                project_id = %app.paths.project_id,
                task_id = %locator.task_id,
                run_id = %locator.run_id,
                round_id = %locator.round_id,
                node_id = %locator.node_id,
                attempt_id = %locator.attempt_id,
                turn_id = %completion.turn_id,
                %error,
                "failed to settle completed queued conversation prompt"
            );
        }
    }
    let queue = match load_prompt_queue(&attempt_dir) {
        Ok(queue) => queue,
        Err(error) => {
            warn!(
                project_id = %app.paths.project_id,
                task_id = %locator.task_id,
                run_id = %locator.run_id,
                round_id = %locator.round_id,
                node_id = %locator.node_id,
                attempt_id = %locator.attempt_id,
                %error,
                "failed to load conversation prompt queue for automatic dispatch"
            );
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
        let attempt_dir = locator.attempt_dir(&app);
        if client::prompt_activity(&attempt_dir).is_some() {
            emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
            return;
        }
        let claimed = match claim_next_for_auto_dispatch(&attempt_dir, expected_revision) {
            Ok(AutoClaimResult::Claimed(item)) => item,
            Ok(
                AutoClaimResult::Empty | AutoClaimResult::Preempted | AutoClaimResult::Suspended,
            ) => {
                emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
                return;
            }
            Err(error) => {
                warn!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    round_id = %locator.round_id,
                    node_id = %locator.node_id,
                    attempt_id = %locator.attempt_id,
                    expected_revision,
                    %error,
                    "failed to claim queued conversation prompt for automatic dispatch"
                );
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
            if let Err(error) = release_queued_prompt(&attempt_dir, &claimed.id) {
                warn!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    round_id = %locator.round_id,
                    node_id = %locator.node_id,
                    attempt_id = %locator.attempt_id,
                    item_id = %claimed.id,
                    %error,
                    "failed to release queued conversation prompt after dispatch suspension"
                );
            }
            emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), false);
            return;
        }
        emit_deferred_turn_completion(&app, &locator, completed_turn.as_ref(), true);
        let queued_turn_app = configure_conversation_runtime_callbacks(
            queued_user_turn_app(&app),
            app_handle.clone(),
            project_id.clone(),
        );
        if let Err(error) = send_acp_prompt_with_configured_app(
            app_handle.clone(),
            queued_turn_app,
            project_id.clone(),
            locator.task_id.clone(),
            locator.run_id.clone(),
            locator.round_id.clone(),
            locator.node_id.clone(),
            locator.attempt_id.clone(),
            ConversationPromptInput {
                display_text: claimed.content.clone(),
                quotes: claimed.quotes.clone(),
            },
            Some(claimed.prompt_id.clone()),
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
            (!claimed.attachment_paths.is_empty()).then_some(claimed.attachment_paths.clone()),
        )
        .await
        {
            warn!(
                project_id = %app.paths.project_id,
                task_id = %locator.task_id,
                run_id = %locator.run_id,
                round_id = %locator.round_id,
                node_id = %locator.node_id,
                attempt_id = %locator.attempt_id,
                turn_id = %claimed.prompt_id,
                error_code = %error.code,
                "automatically dispatched conversation prompt failed"
            );
        }
        if let Err(error) = settle_dispatching_prompt(&attempt_dir, &claimed.prompt_id) {
            warn!(
                project_id = %app.paths.project_id,
                task_id = %locator.task_id,
                run_id = %locator.run_id,
                round_id = %locator.round_id,
                node_id = %locator.node_id,
                attempt_id = %locator.attempt_id,
                turn_id = %claimed.prompt_id,
                %error,
                "failed to settle automatically dispatched conversation prompt"
            );
        }
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
    pub turn_id: Option<String>,
    pub revision: Option<u64>,
    pub operation_id: Option<String>,
    pub session: Option<AcpSessionVm>,
    pub run: Option<RunSummaryVm>,
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
    #[serde(skip)]
    admission_was_terminal: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptQueueMutationVm {
    pub lifecycle: Option<ConversationAttemptLifecycleVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationQueuedPromptDraftVm {
    pub content: String,
    pub quotes: Vec<UserPromptQuote>,
    pub attachment_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPromptQueueRestoreVm {
    pub draft: ConversationQueuedPromptDraftVm,
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

    fn from_runtime_workspace_access(error: RuntimeWorkspaceAccessError) -> Self {
        Self::new(error.code(), error.params())
    }
}

async fn run_runtime_workspace_validation<T, F>(operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    spawn_blocking_command(operation).await
}

pub(crate) async fn validate_runtime_workspace_for_command(
    project_id: &str,
    workspace_path: &str,
) -> CommandResult<()> {
    let project_id = project_id.to_string();
    let workspace_path = workspace_path.to_string();
    run_runtime_workspace_validation(move || {
        validate_runtime_workspace_access(&project_id, &workspace_path)
            .map_err(CommandErrorVm::from_runtime_workspace_access)
    })
    .await
}

pub(crate) async fn prepare_app_exit_inner(
    app_handle: &AppHandle,
    state: &DesktopState,
) -> AppExitPreparationVm {
    let mut result = AppExitPreparationVm::default();

    // Close the process-wide admission gate before scheduler shutdown. A run
    // start either appears in this snapshot or observes ShuttingDown; no
    // registry lock is held while scheduler, file, provider, or SQLite work runs.
    let active_runtime_candidates = match state.runtime_recovery().begin_shutdown() {
        Ok(candidates) => candidates,
        Err(error) => {
            result.record_warning("app-exit.runtime-gate-failed", &error);
            Vec::new()
        }
    };

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
            let runtime_recovery = state.runtime_recovery();
            match tauri::async_runtime::spawn_blocking(move || {
                let mut failure_count = 0usize;
                for candidate in active_runtime_candidates {
                    let workspace_root = camino::Utf8PathBuf::from(&candidate.workspace_path);
                    let paths = gold_band::storage::GoldBandPaths::new(workspace_root.clone());
                    if paths.project_id != candidate.project_id
                        || paths.validate_project_manifest().is_err()
                    {
                        failure_count += 1;
                        warn!(
                            project_id = %candidate.project_id,
                            task_id = %candidate.task_id,
                            run_id = %candidate.run_id,
                            "active runtime candidate workspace identity changed during exit"
                        );
                        continue;
                    }
                    let workspace_app =
                        runtime_app.with_repo_root(workspace_root, runtime_app.config.clone());
                    if let Err(error) = workspace_app.run_pause(
                        &candidate.task_id,
                        &candidate.run_id,
                        PauseReason::ProcessInterrupted,
                    ) {
                        failure_count += 1;
                        warn!(
                            error = %error,
                            project_id = %candidate.project_id,
                            task_id = %candidate.task_id,
                            run_id = %candidate.run_id,
                            "active runtime pause failed during exit"
                        );
                        continue;
                    }
                    if let Err(error) = runtime_recovery.consume_persisted_candidate(&candidate) {
                        failure_count += 1;
                        warn!(
                            error = %error,
                            project_id = %candidate.project_id,
                            task_id = %candidate.task_id,
                            run_id = %candidate.run_id,
                            "active runtime candidate cleanup failed during exit"
                        );
                    }
                }
                runtime_app.close_active_runtime_connections()?;
                if failure_count > 0 {
                    anyhow::bail!("{failure_count} active runtime exit operations failed");
                }
                Ok(())
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
    #[serde(default)]
    pub model_bindings: gold_band::workflow_model_binding::WorkflowModelBindings,
    pub workflow_template_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowInputVm {
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: gold_band::workflow_model_binding::WorkflowModelBindings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowTemplateInputVm {
    pub name: String,
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: gold_band::workflow_model_binding::WorkflowModelBindings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkflowTemplateInputVm {
    pub workflow: WorkflowDsl,
    #[serde(default)]
    pub model_bindings: gold_band::workflow_model_binding::WorkflowModelBindings,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBindingUsageVm {
    pub workflow_template_count: usize,
    pub task_count: usize,
    pub scheduled_task_count: usize,
    pub unknown_task_count: usize,
    pub unknown_scheduled_task_count: usize,
}

fn agent_usage_workspace_apps(
    context: &crate::state::DesktopContext,
    workspaces: &[gold_band::config::ConversationWorkspaceEntry],
) -> anyhow::Result<Vec<App>> {
    let current_app = context.app();
    let mut seen_project_ids = BTreeSet::new();
    let mut apps = Vec::new();
    for workspace in workspaces {
        let app = app_for_workspace(context, &workspace.workspace_path)?;
        let project_key = if cfg!(windows) {
            app.paths.project_id.to_ascii_lowercase()
        } else {
            app.paths.project_id.clone()
        };
        if seen_project_ids.insert(project_key) {
            apps.push(app);
        }
    }
    let current_project_key = if cfg!(windows) {
        current_app.paths.project_id.to_ascii_lowercase()
    } else {
        current_app.paths.project_id.clone()
    };
    if seen_project_ids.insert(current_project_key) {
        apps.push(current_app);
    }
    Ok(apps)
}

fn collect_agent_binding_usage(
    agent_id: &ManagedAgentId,
    templates: &WorkflowTemplateStore,
    workspace_apps: &[App],
) -> anyhow::Result<AgentBindingUsageVm> {
    let agent_id = agent_id.as_str();
    let workflow_template_count = templates
        .templates
        .iter()
        .filter(|template| {
            workflow_references_agent(&template.workflow, &template.model_bindings, agent_id)
        })
        .count();
    let mut task_count = 0;
    let mut scheduled_task_count = 0;
    let mut unknown_task_count = 0;
    let mut unknown_scheduled_task_count = 0;
    for app in workspace_apps {
        let tasks_dir = app.paths.tasks_dir();
        if tasks_dir.exists() {
            for entry in fs::read_dir(tasks_dir.as_std_path())? {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        unknown_task_count += 1;
                        continue;
                    }
                };
                let path = entry.path();
                if !path.is_dir() || !path.join("task.json").exists() {
                    continue;
                }
                let Some(task_id) = path.file_name().and_then(|value| value.to_str()) else {
                    unknown_task_count += 1;
                    continue;
                };
                match app.task_authoring_workflow(task_id) {
                    Ok(authoring)
                        if workflow_references_agent(
                            &authoring.workflow,
                            &authoring.model_bindings,
                            agent_id,
                        ) =>
                    {
                        task_count += 1;
                    }
                    Ok(_) => {}
                    Err(_) => unknown_task_count += 1,
                }
            }
        }
        let scheduler_db_path = app.paths.scheduler_db_path();
        if !scheduler_db_path.exists() {
            continue;
        }
        let scan = ScheduledTaskDatabase::open(scheduler_db_path)?
            .scan_job_definitions(&app.paths.project_id)?;
        unknown_scheduled_task_count += scan.invalid_count;
        for definition in scan.definitions {
            match scheduled_task_references_agent(&definition, agent_id) {
                Ok(true) => scheduled_task_count += 1,
                Ok(false) => {}
                Err(_) => unknown_scheduled_task_count += 1,
            }
        }
    }
    Ok(AgentBindingUsageVm {
        workflow_template_count,
        task_count,
        scheduled_task_count,
        unknown_task_count,
        unknown_scheduled_task_count,
    })
}

fn workflow_references_agent(
    workflow: &WorkflowDsl,
    model_bindings: &gold_band::workflow_model_binding::WorkflowModelBindings,
    agent_id: &str,
) -> bool {
    model_bindings
        .bindings
        .iter()
        .any(|binding| binding.agent_id == agent_id)
        || workflow.nodes.iter().any(|node| {
            providers_for_node(node)
                .iter()
                .any(|provider| provider == agent_id)
        })
}

fn auto_authoring_references_agent(
    authoring: &gold_band::scheduler::AutoAuthoringIdentity,
    agent_id: &str,
) -> bool {
    let agent_type = authoring.agent_type.trim();
    let agent_strategy = authoring.agent_strategy.trim();
    let primary_agent = if matches!(agent_type, "fixed" | "dynamic")
        && !matches!(agent_strategy, "fixed" | "dynamic")
    {
        // Older snapshots wrote these constructor arguments in reverse order.
        agent_strategy
    } else {
        agent_type
    };
    primary_agent == agent_id
        || authoring.bootstrap_agent_type.as_deref() == Some(agent_id)
        || authoring
            .available_agent_types
            .iter()
            .any(|available| available == agent_id)
}

fn scheduled_task_references_agent(
    definition: &gold_band::scheduler::ScheduledTaskDefinition,
    agent_id: &str,
) -> anyhow::Result<bool> {
    if definition.content_snapshot.direct_agent_id.as_deref() == Some(agent_id) {
        return Ok(true);
    }
    if definition
        .content_snapshot
        .auto_authoring
        .as_ref()
        .is_some_and(|authoring| auto_authoring_references_agent(authoring, agent_id))
    {
        return Ok(true);
    }
    let Some(value) = definition.content_snapshot.workflow_authoring.clone() else {
        return Ok(false);
    };
    let authoring = serde_json::from_value::<
        gold_band::workflow_model_binding::TaskAuthoringWorkflowCompat,
    >(value)?
    .into_current()
    .0;
    Ok(workflow_references_agent(
        &authoring.workflow,
        &authoring.model_bindings,
        agent_id,
    ))
}

#[tauri::command]
pub async fn get_agent_binding_usage(
    state: State<'_, DesktopState>,
    agent_type: String,
) -> CommandResult<AgentBindingUsageVm> {
    let agent_id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
    let context = state.context().map_err(command_error)?;
    spawn_blocking_command(move || {
        let app = context.app();
        let app_state = app.load_state().map_err(command_error)?;
        let templates = app.workflow_templates().map_err(command_error)?;
        let workspace_apps =
            agent_usage_workspace_apps(&context, &app_state.conversation_workspaces)
                .map_err(command_error)?;
        collect_agent_binding_usage(&agent_id, &templates, &workspace_apps).map_err(command_error)
    })
    .await
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
    let app = state.app().map_err(command_error)?;
    let background_app = app.clone_for_background();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        let task_input = CreateTaskInput {
            title: input.title,
            description: input.description,
            requirement_file_name: input.requirement_file_name,
            requirement_content: input.requirement_content,
            workflow: input.workflow.clone(),
            workflow_template_id: input.workflow_template_id,
        };
        background_app.create_task_from_requirement_with_bindings(
            task_input,
            input.workflow,
            input.model_bindings,
        )
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
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
    app.save_task_workflow_with_bindings(&task_id, input.workflow, input.model_bindings)
        .map_err(command_error)?;
    workflow_vm(&app, &task_id).map_err(command_error)
}

#[tauri::command]
pub fn get_workflow(
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
) -> CommandResult<WorkflowVm> {
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;
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
    let app = state.app().map_err(command_error)?;
    app.save_workflow_template_with_bindings(input.name, input.workflow, input.model_bindings)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_workflow_template(
    state: State<'_, DesktopState>,
    template_id: String,
    input: UpdateWorkflowTemplateInputVm,
) -> CommandResult<WorkflowTemplateStore> {
    let app = state.app().map_err(command_error)?;
    if app
        .workflow_templates()
        .map_err(command_error)?
        .templates
        .iter()
        .find(|template| template.id == template_id)
        .is_some_and(|template| template.is_built_in)
    {
        app.update_built_in_workflow_template_bindings(&template_id, input.model_bindings)
    } else {
        app.update_workflow_template_with_bindings(
            &template_id,
            input.workflow,
            input.model_bindings,
        )
    }
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
    let _ = state.record_heartbeat_activity();
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
pub async fn get_git_branch_picker_snapshot(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
) -> CommandResult<gold_band::git::GitBranchPickerSnapshot> {
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
            .branch_picker_snapshot(&workspace.workspace_path)
            .map_err(command_error)
    })
    .await
}

#[tauri::command]
pub async fn change_git_branch(
    state: State<'_, DesktopState>,
    project_id: String,
    workspace_path: Option<String>,
    input: gold_band::git::GitBranchChangeRequest,
) -> CommandResult<gold_band::git::GitBranchPickerSnapshot> {
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
            .change_branch(&workspace.workspace_path, &input)
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
                before_path,
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
                    before_path.as_deref(),
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
    monitor_runtime.stop(&project_id, &identity.common_dir, &identity.workspace_path)?;
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
pub async fn continue_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
) -> CommandResult<RunSummaryVm> {
    let _ = state.record_heartbeat_activity();
    let app = resolve_runtime_command_app_with_emitters(
        &app_handle,
        state.inner(),
        project_id.as_deref(),
    )
    .await?;
    app.record_metrics_resume_cause(
        &task_id,
        &run_id,
        gold_band::app::observability::ResumeCause::ManualContinue,
    );
    let result = app.run_continue_background(&task_id, &run_id, None, None);
    if result.is_err() {
        app.clear_metrics_resume_cause(
            &task_id,
            &run_id,
            gold_band::app::observability::ResumeCause::ManualContinue,
        );
    }
    result.map(run_summary_vm).map_err(command_error)
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
    input: Option<ConversationPromptInput>,
    prompt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let log_project_id = project_id.clone();
    let log_task_id = task_id.clone();
    let log_run_id = run_id.clone();
    let log_round_id = round_id.clone();
    let log_node_id = node_id.clone();
    let log_attempt_id = attempt_id.clone();
    let log_outer_node_id = outer_node_id.clone();
    let log_outer_attempt_id = outer_attempt_id.clone();
    let result = continue_conversation_runtime_inner(
        app_handle,
        state,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        input,
        prompt_id,
        attachment_paths,
    )
    .await;
    match &result {
        Ok(value) => info!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            kind = %value.kind,
            "conversation runtime continue accepted"
        ),
        Err(error) => warn!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            error_code = %error.code,
            "conversation runtime continue failed"
        ),
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn continue_conversation_runtime_inner(
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
    input: Option<ConversationPromptInput>,
    prompt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let app = resolve_runtime_command_app_with_emitters(
        &app_handle,
        state.inner(),
        project_id.as_deref(),
    )
    .await?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    if let Some(input) = input.as_ref() {
        validate_conversation_prompt_input(input, attachment_paths.as_deref())?;
    }
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
        let attachment_paths = attachment_paths.unwrap_or_default();
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
                prompt_id,
                input,
                attachment_paths,
                model_override,
                permission_mode_override,
            )
        } else {
            app.run_continue_background_with_config_overrides(
                &locator.task_id,
                &locator.run_id,
                prompt_id,
                input,
                attachment_paths,
                model_override,
                permission_mode_override,
            )
        }
        .map(run_summary_vm)
        .map_err(command_error)?;
        Ok(ConversationPromptSubmitVm {
            kind: "runtime-continue-started".to_string(),
            turn_id: None,
            revision: None,
            operation_id: None,
            session: None,
            run: Some(run),
            lifecycle: runtime_continue_started_lifecycle_for_locator(&app, &locator),
            admission_was_terminal: false,
        })
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn recover_conversation_runtime(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    expected_revision: u64,
) -> CommandResult<ConversationPromptSubmitVm> {
    let app = resolve_runtime_command_app_with_emitters(
        &app_handle,
        state.inner(),
        project_id.as_deref(),
    )
    .await?;
    let locator = AttemptLocator::new(task_id, run_id, round_id, node_id, attempt_id, None, None);
    let app = app.clone_for_background();
    spawn_blocking_command(move || {
        let run = app
            .run_recover_completed_background(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
                expected_revision,
            )
            .map(run_summary_vm)
            .map_err(command_error)?;
        Ok(ConversationPromptSubmitVm {
            kind: "runtime-recovery-started".to_string(),
            turn_id: None,
            revision: None,
            operation_id: None,
            session: None,
            run: Some(run),
            lifecycle: runtime_continue_started_lifecycle_for_locator(&app, &locator),
            admission_was_terminal: false,
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
    let log_project_id = project_id.clone();
    let log_task_id = task_id.clone();
    let log_run_id = run_id.clone();
    let log_round_id = round_id.clone();
    let log_node_id = node_id.clone();
    let log_attempt_id = attempt_id.clone();
    let log_outer_node_id = outer_node_id.clone();
    let log_outer_attempt_id = outer_attempt_id.clone();
    let result = stop_active_session_inner(
        app_handle,
        state,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    )
    .await;
    match &result {
        Ok(value) => info!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            operation_id = %value.operation_id,
            status = %value.status,
            "conversation session stop accepted"
        ),
        Err(error) => warn!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            error_code = %error.code,
            "conversation session stop failed"
        ),
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn stop_active_session_inner(
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
    let operation_id = Uuid::new_v4().to_string();
    let control_operation_id = operation_id.clone();
    let control_app = app.clone_for_background();
    let control_locator = locator.clone();
    let (attempt_dir, stop_turn_id, accepted, current_run, lifecycle) =
        spawn_blocking_command(move || {
            let (attempt_dir, stop_turn_id, accepted) =
                persist_active_session_stop(&control_app, &control_locator, &control_operation_id)?;
            if direct_mode && accepted {
                request_auto_dispatch_suspension(&attempt_dir).map_err(command_error)?;
                let queue = load_prompt_queue(&attempt_dir).map_err(command_error)?;
                if !queue.items.is_empty() {
                    suspend_auto_dispatch(&attempt_dir).map_err(command_error)?;
                }
            }
            let current_run = control_app
                .run_status(&control_locator.task_id, &control_locator.run_id)
                .map_err(command_error)?;
            let lifecycle = lifecycle_for_locator(&control_app, &control_locator);
            Ok((attempt_dir, stop_turn_id, accepted, current_run, lifecycle))
        })
        .await?;

    spawn_active_session_stop_cleanup(
        app_handle,
        app.clone_for_background(),
        project_id,
        locator.clone(),
        attempt_dir,
        stop_turn_id,
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
        status: if accepted { "accepted" } else { "no-op" }.to_string(),
        kind: if accepted {
            "stop-accepted"
        } else {
            "stop-noop"
        }
        .to_string(),
        run: Some(run_summary_vm(current_run)),
        session: None,
        lifecycle,
    })
}

#[tauri::command]
pub async fn submit_manual_check(
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
    let _ = state.record_heartbeat_activity();
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
    let submission_lease = app
        .reserve_manual_check_submission(&task_id, &run_id, &round_id, &node_id, &attempt_id)
        .map_err(command_error)?;
    let resumed_occurrence_id = resume_scheduled_interaction(
        state.inner(),
        &app,
        &task_id,
        &run_id,
        &round_id,
        &attempt_id,
    )
    .await?;
    let app = app
        .into_inner()
        .with_scheduled_occurrence_id(resumed_occurrence_id);
    app.submit_manual_check_background(
        &task_id,
        &run_id,
        &round_id,
        &node_id,
        &attempt_id,
        outcome,
        submission_lease,
    )
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
    let _ = state.record_heartbeat_activity();
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

const FRONTEND_ERROR_MESSAGE_MAX_CHARS: usize = 4_096;
const FRONTEND_ERROR_STACK_MAX_CHARS: usize = 16_384;
const FRONTEND_ERROR_CONTEXT_MAX_CHARS: usize = 2_048;
const FRONTEND_ERROR_ELEMENT_MAX_CHARS: usize = 1_024;
const FRONTEND_ERROR_TIMESTAMP_MAX_CHARS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum FrontendErrorKindInput {
    WindowError,
    UnhandledRejection,
    ReactUncaught,
}

impl FrontendErrorKindInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::WindowError => "window-error",
            Self::UnhandledRejection => "unhandled-rejection",
            Self::ReactUncaught => "react-uncaught",
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontendErrorReportInput {
    kind: FrontendErrorKindInput,
    message: String,
    stack: Option<String>,
    component_stack: Option<String>,
    source: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    active_element: Option<String>,
    last_pointer_target: Option<String>,
    last_pointer_at: Option<String>,
    pathname: Option<String>,
    user_agent: Option<String>,
}

impl FrontendErrorReportInput {
    fn normalize(mut self) -> Self {
        self.message =
            truncate_frontend_error_field(self.message, FRONTEND_ERROR_MESSAGE_MAX_CHARS);
        self.stack =
            truncate_optional_frontend_error_field(self.stack, FRONTEND_ERROR_STACK_MAX_CHARS);
        self.component_stack = truncate_optional_frontend_error_field(
            self.component_stack,
            FRONTEND_ERROR_STACK_MAX_CHARS,
        );
        self.source = sanitize_frontend_error_source(self.source);
        self.active_element = truncate_optional_frontend_error_field(
            self.active_element,
            FRONTEND_ERROR_ELEMENT_MAX_CHARS,
        );
        self.last_pointer_target = truncate_optional_frontend_error_field(
            self.last_pointer_target,
            FRONTEND_ERROR_ELEMENT_MAX_CHARS,
        );
        self.last_pointer_at = truncate_optional_frontend_error_field(
            self.last_pointer_at,
            FRONTEND_ERROR_TIMESTAMP_MAX_CHARS,
        );
        self.pathname =
            truncate_optional_frontend_error_field(self.pathname, FRONTEND_ERROR_CONTEXT_MAX_CHARS);
        self.user_agent = truncate_optional_frontend_error_field(
            self.user_agent,
            FRONTEND_ERROR_CONTEXT_MAX_CHARS,
        );
        self
    }
}

fn truncate_frontend_error_field(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value
    } else {
        value.chars().take(max_chars).collect()
    }
}

fn truncate_optional_frontend_error_field(
    value: Option<String>,
    max_chars: usize,
) -> Option<String> {
    value.map(|value| truncate_frontend_error_field(value, max_chars))
}

fn sanitize_frontend_error_source(value: Option<String>) -> Option<String> {
    value.map(|value| {
        let without_query = value
            .find(|character| character == '?' || character == '#')
            .map_or(value.as_str(), |index| &value[..index]);
        truncate_frontend_error_field(without_query.to_string(), FRONTEND_ERROR_CONTEXT_MAX_CHARS)
    })
}

#[tauri::command]
pub fn report_frontend_error(input: FrontendErrorReportInput) -> CommandResult<()> {
    let report = input.normalize();
    tracing::error!(
        target: "gold_band::frontend",
        kind = report.kind.as_str(),
        message = %report.message,
        stack = report.stack.as_deref().unwrap_or(""),
        component_stack = report.component_stack.as_deref().unwrap_or(""),
        source = report.source.as_deref().unwrap_or(""),
        line = ?report.line,
        column = ?report.column,
        active_element = report.active_element.as_deref().unwrap_or(""),
        last_pointer_target = report.last_pointer_target.as_deref().unwrap_or(""),
        last_pointer_at = report.last_pointer_at.as_deref().unwrap_or(""),
        pathname = report.pathname.as_deref().unwrap_or(""),
        user_agent = report.user_agent.as_deref().unwrap_or(""),
        "frontend fatal error reported"
    );
    Ok(())
}

const WEBVIEW_USER_AGENT_MAX_CHARS: usize = 2_048;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebviewSupportTierInput {
    Unsupported,
    Compatible,
    Full,
}

impl WebviewSupportTierInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Compatible => "compatible",
            Self::Full => "full",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebviewThemeRenderingInput {
    FallbackTokens,
    ModernCss,
}

impl WebviewThemeRenderingInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::FallbackTokens => "fallback-tokens",
            Self::ModernCss => "modern-css",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebviewResponsiveLayoutInput {
    Measured,
    ContainerQuery,
}

impl WebviewResponsiveLayoutInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::ContainerQuery => "container-query",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebviewCodeHighlightingInput {
    Plain,
    Wasm,
}

impl WebviewCodeHighlightingInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Wasm => "wasm",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WebviewVisualMaterialInput {
    Solid,
    Native,
}

impl WebviewVisualMaterialInput {
    fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Native => "native",
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebviewCapabilitiesInput {
    regexp_lookbehind: bool,
    css_color_mix: bool,
    css_container_queries: bool,
    css_has_selector: bool,
    css_backdrop_filter: bool,
    css_oklch: bool,
    css_grid: bool,
    css_custom_properties: bool,
    resize_observer: bool,
    structured_clone: bool,
    web_assembly: bool,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebviewFeaturePolicyInput {
    tier: WebviewSupportTierInput,
    theme_rendering: WebviewThemeRenderingInput,
    responsive_layout: WebviewResponsiveLayoutInput,
    code_highlighting: WebviewCodeHighlightingInput,
    visual_material: WebviewVisualMaterialInput,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebviewEnvironmentReportInput {
    user_agent: String,
    capabilities: WebviewCapabilitiesInput,
    policy: WebviewFeaturePolicyInput,
}

impl WebviewEnvironmentReportInput {
    fn normalize(mut self) -> Self {
        self.user_agent =
            truncate_frontend_error_field(self.user_agent, WEBVIEW_USER_AGENT_MAX_CHARS);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebviewRuntimeFactsVm {
    platform: String,
    architecture: String,
    os_version: Option<String>,
    webkit_bundle_version: Option<String>,
}

#[cfg(target_os = "macos")]
fn system_plist_string(path: &Path, key: &str) -> Option<String> {
    let value = plist::Value::from_file(path).ok()?;
    value
        .as_dictionary()?
        .get(key)?
        .as_string()
        .map(ToOwned::to_owned)
}

fn webview_runtime_facts() -> WebviewRuntimeFactsVm {
    #[cfg(target_os = "macos")]
    let (os_version, webkit_bundle_version) = (
        system_plist_string(
            Path::new("/System/Library/CoreServices/SystemVersion.plist"),
            "ProductVersion",
        ),
        system_plist_string(
            Path::new("/System/Library/Frameworks/WebKit.framework/Resources/Info.plist"),
            "CFBundleVersion",
        ),
    );
    #[cfg(not(target_os = "macos"))]
    let (os_version, webkit_bundle_version) = (None, None);

    WebviewRuntimeFactsVm {
        platform: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        os_version,
        webkit_bundle_version,
    }
}

#[tauri::command]
pub async fn report_webview_environment(
    input: WebviewEnvironmentReportInput,
) -> CommandResult<WebviewRuntimeFactsVm> {
    let report = input.normalize();
    let facts = spawn_blocking_command(|| Ok(webview_runtime_facts())).await?;
    tracing::info!(
        target: "gold_band::frontend",
        code = "webview.environment.detected",
        platform = %facts.platform,
        architecture = %facts.architecture,
        os_version = facts.os_version.as_deref().unwrap_or(""),
        webkit_bundle_version = facts.webkit_bundle_version.as_deref().unwrap_or(""),
        tier = report.policy.tier.as_str(),
        theme_rendering = report.policy.theme_rendering.as_str(),
        responsive_layout = report.policy.responsive_layout.as_str(),
        code_highlighting = report.policy.code_highlighting.as_str(),
        visual_material = report.policy.visual_material.as_str(),
        regexp_lookbehind = report.capabilities.regexp_lookbehind,
        css_color_mix = report.capabilities.css_color_mix,
        css_container_queries = report.capabilities.css_container_queries,
        css_has_selector = report.capabilities.css_has_selector,
        css_backdrop_filter = report.capabilities.css_backdrop_filter,
        css_oklch = report.capabilities.css_oklch,
        css_grid = report.capabilities.css_grid,
        css_custom_properties = report.capabilities.css_custom_properties,
        resize_observer = report.capabilities.resize_observer,
        structured_clone = report.capabilities.structured_clone,
        web_assembly = report.capabilities.web_assembly,
        user_agent = %report.user_agent,
        "webview environment detected"
    );
    Ok(facts)
}

/// Frontend activity signal: pointerdown, keydown, or business command.
#[tauri::command]
pub fn record_activity(state: State<'_, DesktopState>) -> CommandResult<()> {
    state.record_heartbeat_activity().map_err(command_error)
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
    let _ = state.reevaluate_heartbeat_config();
    Ok(metrics_settings(&updated_context.config))
}

pub(crate) fn acp_live_update_emitter(
    app_handle: AppHandle,
    project_id: Option<String>,
    notification_app: Option<App>,
    lifecycle_bus: Option<gold_band::app::observability::RuntimeLifecycleBus>,
) -> Arc<
    dyn Fn(
            gold_band::app::AcpLiveEventContext,
            AcpUiEvent,
            AcpLiveTimelinePosition,
        ) -> anyhow::Result<()>
        + Send
        + Sync,
> {
    Arc::new(move |context, mut event, timeline_position| {
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
            context.task_uuid.clone(),
            &context.task_id,
            &context.run_id,
            &context.round_id,
            &context.node_id,
            &context.attempt_id,
            context.outer_node_id.clone(),
            context.outer_attempt_id.clone(),
            event,
            timeline_position,
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
    let event_id = request_scoped_intervention_event_id(
        project_id,
        context,
        event,
        PERMISSION_REQUESTED_DEDUP_SUFFIX,
    );
    lifecycle_bus.emit(RuntimeLifecycleEvent::InterventionRequested {
        event_id: event_id.clone(),
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
    if let Some(app) = app {
        emit_request_intervention_metrics(
            app,
            context,
            &event_id,
            RuntimeInterventionKind::PermissionRequested,
        );
    }
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
    let event_id = request_scoped_intervention_event_id(
        project_id,
        context,
        event,
        ELICITATION_REQUESTED_DEDUP_SUFFIX,
    );
    lifecycle_bus.emit(RuntimeLifecycleEvent::InterventionRequested {
        event_id: event_id.clone(),
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
    if let Some(app) = app {
        emit_request_intervention_metrics(
            app,
            context,
            &event_id,
            RuntimeInterventionKind::ElicitationRequested,
        );
    }
}

fn emit_request_intervention_metrics(
    app: &App,
    context: &gold_band::app::AcpLiveEventContext,
    request_id: &str,
    kind: RuntimeInterventionKind,
) {
    if let Some(sender) = direct_metrics_sender() {
        let _ = sender.try_send(DirectMetricsJob::InterventionRequested {
            context: context.clone(),
            request_id: request_id.to_string(),
            kind,
            repo_root: app.paths.repo_root.to_string(),
        });
    }
}

fn build_request_intervention_metrics(
    app: &App,
    context: &gold_band::app::AcpLiveEventContext,
    request_id: &str,
    kind: RuntimeInterventionKind,
) {
    if !app.metrics_collection_enabled() {
        return;
    }
    let Ok(run) = app.run_status(&context.task_id, &context.run_id) else {
        return;
    };
    let (Some(task_uuid), Some(run_uuid)) = (run.task_uuid.clone(), run.uuid.clone()) else {
        return;
    };
    let is_direct =
        gold_band::app::direct_conversation_agent_label(app, &context.task_id).is_some();
    let is_auto = !is_direct && context.outer_node_id.is_some();
    let active_turn = if is_direct {
        app.active_metrics_turn(&format!("direct:{task_uuid}"))
    } else {
        None
    };
    let execution_id = active_turn
        .as_ref()
        .map(|turn| turn.execution_id.clone())
        .unwrap_or_else(|| task_uuid.clone());
    let event_revision;
    let collection_state_recovered;
    let _state = if let Some(active_turn) = active_turn.as_ref() {
        let attempt_path = app
            .paths
            .run_dir(&context.task_id, &context.run_id)
            .join("observability")
            .join(&execution_id)
            .join(&active_turn.attempt_id)
            .join(gold_band::app::observability::OBSERVABILITY_SNAPSHOT_FILE);
        let attempt_state =
            app.update_observability_state(&active_turn.attempt_id, attempt_path, |state| {
                match kind {
                    RuntimeInterventionKind::PermissionRequested => {
                        state.record_permission(request_id)
                    }
                    RuntimeInterventionKind::ElicitationRequested => {
                        state.record_elicitation(request_id)
                    }
                    _ => {}
                }
                state.next_revision();
            });
        event_revision = attempt_state.event_revision;
        collection_state_recovered = attempt_state.collection_state_recovered;
        attempt_state
    } else {
        let path = app
            .paths
            .run_dir(&context.task_id, &context.run_id)
            .join("observability")
            .join(&execution_id)
            .join(gold_band::app::observability::OBSERVABILITY_SNAPSHOT_FILE);
        let state = app.update_observability_state(&execution_id, path, |state| {
            match kind {
                RuntimeInterventionKind::PermissionRequested => state.record_permission(request_id),
                RuntimeInterventionKind::ElicitationRequested => {
                    state.record_elicitation(request_id)
                }
                _ => {}
            }
            state.next_revision();
        });
        event_revision = state.event_revision;
        collection_state_recovered = state.collection_state_recovered;
        state
    };
    let mut fact = gold_band::app::observability::MetricsLifecycleFact::new(
        gold_band::app::observability::LifecycleEventType::InterventionRequested,
        event_revision,
        current_timestamp(),
        crate::metrics::get_system_username(),
        app.paths.repo_root.to_string(),
        if is_direct {
            gold_band::app::observability::MetricsSessionMode::Direct
        } else if is_auto {
            gold_band::app::observability::MetricsSessionMode::Auto
        } else {
            gold_band::app::observability::MetricsSessionMode::Workflow
        },
        task_uuid,
        if is_direct {
            gold_band::app::observability::ExecutionKind::Turn
        } else if is_auto {
            gold_band::app::observability::ExecutionKind::OuterRun
        } else {
            gold_band::app::observability::ExecutionKind::Run
        },
        execution_id.clone(),
    );
    fact.task_title = app.task_show(&context.task_id).ok().and_then(|t| t.title);
    fact.intervention_kind = Some(match kind {
        RuntimeInterventionKind::PermissionRequested => {
            gold_band::app::observability::MetricsInterventionKind::Permission
        }
        RuntimeInterventionKind::ElicitationRequested => {
            gold_band::app::observability::MetricsInterventionKind::Elicitation
        }
        RuntimeInterventionKind::ManualDecisionRequired => {
            gold_band::app::observability::MetricsInterventionKind::ManualDecision
        }
        RuntimeInterventionKind::RuntimeAbnormal => {
            gold_band::app::observability::MetricsInterventionKind::RuntimeAbnormal
        }
        RuntimeInterventionKind::ErrorBlocked => {
            gold_band::app::observability::MetricsInterventionKind::ErrorBlocked
        }
        RuntimeInterventionKind::ProcessInterrupted => {
            gold_band::app::observability::MetricsInterventionKind::ProcessInterrupted
        }
    });
    if is_direct {
        if let Some(turn) = active_turn {
            fact.attempt_id = Some(turn.attempt_id.clone());
            fact.attempt_index = Some(turn.attempt_index);
        }
    } else {
        apply_intervention_node_context(app, context, &run_uuid, &mut fact, is_auto);
    }
    fact.collection_state_recovered = collection_state_recovered;
    app.emit_lifecycle_event(RuntimeLifecycleEvent::MetricsFact(fact));
}

fn apply_intervention_node_context(
    app: &App,
    context: &gold_band::app::AcpLiveEventContext,
    run_uuid: &str,
    fact: &mut gold_band::app::observability::MetricsLifecycleFact,
    is_auto: bool,
) {
    let round_index = read_json::<gold_band::runtime::RoundState>(&app.paths.round_file(
        &context.task_id,
        &context.run_id,
        &context.round_id,
    ))
    .ok()
    .map(|round| round.index);
    fact.round_index = round_index;

    if is_auto {
        let (Some(outer_node_id), Some(outer_attempt_id)) = (
            context.outer_node_id.as_deref(),
            context.outer_attempt_id.as_deref(),
        ) else {
            return;
        };
        let Ok(graph) =
            read_json::<gold_band::dynamic::DynamicGraphState>(&app.paths.dynamic_graph_file(
                &context.task_id,
                &context.run_id,
                &context.round_id,
                outer_node_id,
                outer_attempt_id,
            ))
        else {
            return;
        };
        let Some(dynamic_node) = graph.nodes.iter().find(|node| node.id == context.node_id) else {
            return;
        };
        fact.attempt_index =
            gold_band::app::observability::attempt_index_from_local_id(&context.attempt_id);
        fact.role_name = Some(dynamic_node.title.clone());
        if let Some(node_uuid) = dynamic_node.uuid.as_deref() {
            fact.node_id = Some(node_uuid.to_string());
            fact.attempt_id =
                gold_band::app::observability::derive_attempt_id(node_uuid, &context.attempt_id);
        }
        return;
    }

    let Ok(node) = read_json::<NodeState>(&app.paths.node_file(
        &context.task_id,
        &context.run_id,
        &context.round_id,
        &context.node_id,
        &context.attempt_id,
    )) else {
        return;
    };
    fact.attempt_index =
        gold_band::app::observability::attempt_index_from_local_id(&node.attempt_id);
    fact.role_name = Some(node_intervention_role_name(&node));
    let Some(node_uuid) = node.uuid.as_deref() else {
        return;
    };
    let logical = round_index
        .and_then(|round_index| {
            gold_band::app::observability::derive_execution_id(
                run_uuid,
                &format!("round:{round_index}:node:{}", node.node_id),
            )
        })
        .unwrap_or_else(|| node_uuid.to_string());
    fact.node_id = Some(logical);
    fact.attempt_id = Some(node_uuid.to_string());
}

fn node_intervention_role_name(node: &NodeState) -> String {
    node.resolved_config
        .get("profileName")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            node.resolved_config
                .get("profile")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            node.resolved_config
                .get("provider")
                .and_then(|value| value.as_str())
        })
        .unwrap_or_else(|| node.node_id.as_str())
        .to_string()
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
        // Session lifecycle is a control-plane event. Publishing it must not
        // rebuild or serialize timeline正文; page data is fetched only by the
        // explicit session query/pagination path.
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
            None,
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
    let activity = conversation_task_prompt_activity_vm(app, task_id);
    let task_uuid = app
        .run_status(task_id, run_id)
        .ok()
        .and_then(|run| run.task_uuid);
    emit_acp_update(
        app_handle,
        Some(app),
        project_id,
        task_uuid,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        session,
        None,
        None,
        activity,
    );
}

fn emit_acp_event_update(
    app_handle: &AppHandle,
    activity_app: Option<&App>,
    project_id: Option<String>,
    task_uuid: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    event: AcpUiEvent,
    timeline_position: AcpLiveTimelinePosition,
) {
    let activity = activity_app.and_then(|app| conversation_task_prompt_activity_vm(app, task_id));
    emit_acp_update(
        app_handle,
        None,
        project_id,
        task_uuid,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
        None,
        Some(event),
        Some(timeline_position),
        activity,
    );
}

fn conversation_task_prompt_activity_vm(
    app: &App,
    task_id: &str,
) -> Option<ConversationTaskActivityVm> {
    client::prompt_activity_under(&app.paths.task_dir(task_id))
        .map(conversation_task_activity_from_prompt)
}

fn acp_timeline_position_fields(
    timeline_position: Option<AcpLiveTimelinePosition>,
) -> (Option<u64>, Option<u64>) {
    timeline_position
        .map(|position| (Some(position.generation), position.revision))
        .unwrap_or((None, None))
}

#[allow(clippy::too_many_arguments)]
fn emit_acp_update(
    app_handle: &AppHandle,
    app: Option<&App>,
    project_id: Option<String>,
    task_uuid: Option<String>,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    session: Option<AcpSessionVm>,
    event: Option<AcpUiEvent>,
    timeline_position: Option<AcpLiveTimelinePosition>,
    activity: Option<ConversationTaskActivityVm>,
) {
    let branch_id = event
        .as_ref()
        .map(gold_band::acp::branches::event_branch_id);
    let (timeline_generation, timeline_revision) = acp_timeline_position_fields(timeline_position);
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
    let task_activity_at = app.and_then(|app| {
        crate::view_models_conversation::conversation_task_last_activity_at(app, task_id)
    });
    let _ = app_handle.emit(
        ACP_SESSION_EVENT,
        AcpSessionUpdatedEventVm {
            branch_id,
            timeline_generation,
            timeline_revision,
            project_id,
            task_id: task_id.to_string(),
            task_uuid,
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
            task_activity_at,
        },
    );
}

fn acp_live_event_context(
    task_id: &str,
    task_uuid: Option<String>,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> gold_band::app::AcpLiveEventContext {
    gold_band::app::AcpLiveEventContext {
        task_id: task_id.to_string(),
        task_uuid,
        run_id: run_id.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        outer_node_id,
        outer_attempt_id,
    }
}

#[tauri::command]
pub async fn get_acp_session(
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
    let app = resolve_command_app(state.inner(), project_id.as_deref())?.clone_for_background();
    spawn_blocking_command(move || {
        get_acp_session_from_app(
            app,
            task_id,
            run_id,
            round_id,
            node_id,
            attempt_id,
            query,
            outer_node_id,
            outer_attempt_id,
        )
    })
    .await
}

#[allow(clippy::too_many_arguments)]
fn get_acp_session_from_app(
    app: App,
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

#[tauri::command]
pub async fn resolve_turn_attachment_file(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    workspace_file_runtime: State<'_, crate::workspace_files::WorkspaceFileRuntime>,
    workspace_file_watch_runtime: State<'_, crate::workspace_files::WorkspaceFileWatchRuntime>,
    project_id: String,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    branch_id: String,
    change_set_id: String,
    attachment_id: String,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<crate::workspace_files::ResolvedWorkspaceFileLinkVm> {
    gold_band::acp::branches::validate_conversation_branch_id(&branch_id)
        .map_err(|_| CommandErrorVm::new(ATTACHMENT_ACCESS_DENIED, serde_json::json!({})))?;
    let app = resolve_command_app(state.inner(), Some(&project_id))?.clone_for_background();
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    let path = spawn_blocking_command(move || {
        let store = TurnFileStore::new(locator.attempt_dir(&app), app.config.turn_files.into());
        let change_set = store
            .load_change_set(&change_set_id)
            .map_err(turn_file_command_error)?;
        if change_set.branch_id != branch_id {
            return Err(CommandErrorVm::new(
                ATTACHMENT_ACCESS_DENIED,
                serde_json::json!({}),
            ));
        }
        store
            .resolve_attachment_path(&change_set_id, &attachment_id)
            .map(|path| path.into_std_path_buf())
            .map_err(turn_file_command_error)
    })
    .await?;
    crate::workspace_files::resolve_trusted_file(
        app_handle,
        state.inner(),
        workspace_file_runtime.inner(),
        workspace_file_watch_runtime.inner(),
        &project_id,
        path,
    )
}

fn turn_file_command_error(error: anyhow::Error) -> CommandErrorVm {
    let message = error.to_string();
    let code = if message.starts_with(VERSION_NOT_FOUND) {
        VERSION_NOT_FOUND
    } else if message.starts_with(ATTACHMENT_NOT_FOUND) {
        ATTACHMENT_NOT_FOUND
    } else if message.starts_with(ATTACHMENT_ACCESS_DENIED) {
        ATTACHMENT_ACCESS_DENIED
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
pub fn reorder_conversation_queued_prompts(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    expected_revision: u64,
    ordered_item_ids: Vec<String>,
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
    reorder_queued_prompts(
        &locator.attempt_dir(&app),
        expected_revision,
        ordered_item_ids,
    )
    .map_err(prompt_queue_command_error)?;
    Ok(ConversationPromptQueueMutationVm {
        lifecycle: emit_prompt_queue_lifecycle(&app_handle, &app, project_id, &locator),
    })
}

#[tauri::command]
pub fn restore_conversation_queued_prompt(
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
) -> CommandResult<ConversationPromptQueueRestoreVm> {
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
    let (item, _) = take_queued_prompt(&locator.attempt_dir(&app), &item_id)
        .map_err(prompt_queue_command_error)?;
    Ok(ConversationPromptQueueRestoreVm {
        draft: ConversationQueuedPromptDraftVm {
            content: item.content,
            quotes: item.quotes,
            attachment_paths: item.attachment_paths,
        },
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
    let mut result = submit_conversation_prompt(
        app_handle.clone(),
        state.clone(),
        project_id.clone(),
        locator.task_id.clone(),
        locator.run_id.clone(),
        locator.round_id.clone(),
        locator.node_id.clone(),
        locator.attempt_id.clone(),
        ConversationPromptInput {
            display_text: claimed.content.clone(),
            quotes: claimed.quotes.clone(),
        },
        Some(claimed.prompt_id.clone()),
        locator.outer_node_id.clone(),
        locator.outer_attempt_id.clone(),
        (!claimed.attachment_paths.is_empty()).then_some(claimed.attachment_paths.clone()),
    )
    .await;
    if result
        .as_ref()
        .is_ok_and(|response| response.admission_was_terminal)
    {
        match recover_terminal_dispatch(&attempt_dir, &claimed.id) {
            Ok(TerminalDispatchRecovery::Reclaimed(reclaimed)) => {
                // The prior turn ended before its user prompt reached the
                // canonical timeline. Keep the queue item but use a new turn
                // identity for this explicit retry.
                result = submit_conversation_prompt(
                    app_handle.clone(),
                    state,
                    project_id.clone(),
                    locator.task_id.clone(),
                    locator.run_id.clone(),
                    locator.round_id.clone(),
                    locator.node_id.clone(),
                    locator.attempt_id.clone(),
                    ConversationPromptInput {
                        display_text: reclaimed.content,
                        quotes: reclaimed.quotes,
                    },
                    Some(reclaimed.prompt_id),
                    locator.outer_node_id.clone(),
                    locator.outer_attempt_id.clone(),
                    (!reclaimed.attachment_paths.is_empty()).then_some(reclaimed.attachment_paths),
                )
                .await;
            }
            Ok(TerminalDispatchRecovery::AlreadyAccepted | TerminalDispatchRecovery::Missing) => {
                emit_prompt_queue_lifecycle(&app_handle, &app, project_id.clone(), &locator);
            }
            Err(error) => {
                warn!(%error, item_id = %claimed.id, "failed to recover terminal queued prompt dispatch");
                result = Err(command_error(error));
            }
        }
    }
    if result.is_err() {
        if let Err(error) = release_queued_prompt(&attempt_dir, &claimed.id) {
            warn!(%error, item_id = %claimed.id, "failed to restore queued prompt after submission rejection");
        }
        emit_prompt_queue_lifecycle(&app_handle, &app, project_id, &locator);
    }
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
    input: ConversationPromptInput,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let log_project_id = project_id.clone();
    let log_task_id = task_id.clone();
    let log_run_id = run_id.clone();
    let log_round_id = round_id.clone();
    let log_node_id = node_id.clone();
    let log_attempt_id = attempt_id.clone();
    let log_outer_node_id = outer_node_id.clone();
    let log_outer_attempt_id = outer_attempt_id.clone();
    let result = submit_conversation_prompt_inner(
        app_handle,
        state,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        input,
        prompt_id,
        outer_node_id,
        outer_attempt_id,
        attachment_paths,
    )
    .await;
    match &result {
        Ok(value) => info!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            kind = %value.kind,
            turn_id = ?value.turn_id,
            operation_id = ?value.operation_id,
            revision = ?value.revision,
            "conversation prompt submission accepted"
        ),
        Err(error) => warn!(
            project_id = ?log_project_id,
            task_id = %log_task_id,
            run_id = %log_run_id,
            round_id = %log_round_id,
            node_id = %log_node_id,
            attempt_id = %log_attempt_id,
            outer_node_id = ?log_outer_node_id,
            outer_attempt_id = ?log_outer_attempt_id,
            error_code = %error.code,
            "conversation prompt submission failed"
        ),
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn submit_conversation_prompt_inner(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    input: ConversationPromptInput,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<ConversationPromptSubmitVm> {
    let _ = state.record_heartbeat_activity();
    let app = resolve_runtime_command_app_with_emitters(
        &app_handle,
        state.inner(),
        project_id.as_deref(),
    )
    .await?;
    let locator = AttemptLocator::new(
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    );
    validate_conversation_prompt_input(&input, attachment_paths.as_deref())?;
    let run = app
        .run_status(&locator.task_id, &locator.run_id)
        .map_err(command_error)?;
    let direct_mode = conversation_run_mode(&app, &locator.task_id)
        == Some(gold_band::config::ConversationRunMode::Direct);
    let attempt_dir = locator.attempt_dir(&app);
    if let Some(existing) = existing_conversation_prompt_turn(
        &app,
        &locator,
        prompt_id.as_deref(),
        &input,
        attachment_paths.as_deref().unwrap_or_default(),
    )? {
        let admission_was_terminal = matches!(
            &existing,
            gold_band::acp::events::AcpTurnAdmission::ExistingTerminal(_)
        );
        let header = existing.into_header();
        return Ok(ConversationPromptSubmitVm {
            kind: "acp-session-started".to_string(),
            turn_id: header.turn_id,
            revision: Some(header.revision),
            operation_id: header.operation_id,
            session: None,
            run: None,
            lifecycle: lifecycle_for_locator(&app, &locator),
            admission_was_terminal,
        });
    }
    let live_prompt_active = matches!(
        client::prompt_activity(&attempt_dir),
        Some(
            client::PromptActivity::Starting
                | client::PromptActivity::Accepted
                | client::PromptActivity::Running
        )
    );
    if direct_mode && (live_prompt_active || run.status == RunStatus::Running) {
        let queued = enqueue_prompt(&attempt_dir, input, attachment_paths.unwrap_or_default())
            .map_err(prompt_queue_command_error)?;
        touch_task_activity_at_best_effort(
            &app,
            &locator.task_id,
            &queued.created_at,
            "user-prompt-queued",
        );
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
            if let Err(error) = app.notify_prompt_turn_finished(
                acp_live_event_context(
                    &locator.task_id,
                    run.task_uuid.clone(),
                    &locator.run_id,
                    &locator.round_id,
                    &locator.node_id,
                    &locator.attempt_id,
                    locator.outer_node_id.clone(),
                    locator.outer_attempt_id.clone(),
                ),
                None,
                true,
            ) {
                warn!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    round_id = %locator.round_id,
                    node_id = %locator.node_id,
                    attempt_id = %locator.attempt_id,
                    %error,
                    "failed to dispatch queued conversation prompt after terminal run"
                );
            }
        }
        return Ok(ConversationPromptSubmitVm {
            kind: "queued".to_string(),
            turn_id: None,
            revision: None,
            operation_id: None,
            session: None,
            run: None,
            lifecycle,
            admission_was_terminal: false,
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
    let admission = admit_conversation_prompt_turn(
        &app_handle,
        &app,
        project_id.clone(),
        &locator,
        prompt_id,
        &input,
        attachment_paths.as_deref().unwrap_or_default(),
    )?;
    let header = admission.header().clone();
    let turn_id = header
        .turn_id
        .clone()
        .expect("admitted ACP turn must carry its stable turn identity");
    let lifecycle = lifecycle_for_locator(&app, &locator);
    if admission.started() {
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
    }
    if admission.started() {
        let background_app = app.clone_for_background();
        let background_app_handle = app_handle.clone();
        let background_project_id = project_id.clone();
        let background_locator = locator.clone();
        let failure_turn_id = turn_id.clone();
        let failure_revision = header.revision;
        let failure_operation_id = header.operation_id.clone();
        let background = execute_admitted_acp_prompt_with_configured_app(
            app_handle,
            app,
            project_id,
            locator.task_id.clone(),
            locator.run_id.clone(),
            locator.round_id.clone(),
            locator.node_id.clone(),
            locator.attempt_id.clone(),
            turn_id.clone(),
            failure_revision,
            failure_operation_id,
            locator.outer_node_id.clone(),
            locator.outer_attempt_id.clone(),
        );
        tauri::async_runtime::spawn(async move {
            if let Err(error) = background.await {
                warn!(
                    project_id = %background_app.paths.project_id,
                    task_id = %background_locator.task_id,
                    run_id = %background_locator.run_id,
                    round_id = %background_locator.round_id,
                    node_id = %background_locator.node_id,
                    attempt_id = %background_locator.attempt_id,
                    outer_node_id = ?background_locator.outer_node_id,
                    outer_attempt_id = ?background_locator.outer_attempt_id,
                    error_code = %error.code,
                    turn_id = %failure_turn_id,
                    "accepted ACP prompt failed in background"
                );
            }
            if let Err(error) = settle_dispatching_prompt(
                &background_locator.attempt_dir(&background_app),
                &failure_turn_id,
            ) {
                warn!(
                    project_id = %background_app.paths.project_id,
                    task_id = %background_locator.task_id,
                    run_id = %background_locator.run_id,
                    round_id = %background_locator.round_id,
                    node_id = %background_locator.node_id,
                    attempt_id = %background_locator.attempt_id,
                    turn_id = %failure_turn_id,
                    %error,
                    "failed to settle conversation prompt after background completion"
                );
            }
            emit_prompt_queue_lifecycle(
                &background_app_handle,
                &background_app,
                background_project_id,
                &background_locator,
            );
        });
    }
    Ok(ConversationPromptSubmitVm {
        kind: "acp-session-started".to_string(),
        turn_id: header.turn_id,
        revision: Some(header.revision),
        operation_id: header.operation_id,
        session: None,
        run: None,
        lifecycle,
        admission_was_terminal: matches!(
            admission,
            gold_band::acp::events::AcpTurnAdmission::ExistingTerminal(_)
        ),
    })
}

pub(crate) async fn send_acp_prompt_with_configured_app(
    app_handle: AppHandle,
    app: ConfiguredConversationApp,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    input: ConversationPromptInput,
    prompt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
    attachment_paths: Option<Vec<String>>,
) -> CommandResult<()> {
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
    let admission = admit_conversation_prompt_turn(
        &app_handle,
        &app,
        project_id.clone(),
        &locator,
        prompt_id,
        &input,
        attachment_paths.as_deref().unwrap_or_default(),
    )?;
    if !admission.started() {
        return Ok(());
    }
    let turn_id = admission
        .header()
        .turn_id
        .clone()
        .expect("admitted ACP turn must carry its stable turn identity");
    execute_admitted_acp_prompt_with_configured_app(
        app_handle,
        app,
        project_id,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        turn_id,
        admission.header().revision,
        admission.header().operation_id.clone(),
        outer_node_id,
        outer_attempt_id,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_admitted_acp_prompt_with_configured_app(
    app_handle: AppHandle,
    app: ConfiguredConversationApp,
    project_id: Option<String>,
    task_id: String,
    run_id: String,
    round_id: String,
    node_id: String,
    attempt_id: String,
    turn_id: String,
    expected_revision: u64,
    expected_operation_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
) -> CommandResult<()> {
    let app = app.into_inner();
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
    let lifecycle_path = acp_lifecycle_path(&locator.attempt_dir(&app));
    let submission =
        gold_band::acp::events::read_session_prompt_submission(&lifecycle_path, &turn_id)
            .map_err(command_error)?
            .ok_or_else(|| {
                CommandErrorVm::new(
                    "conversation.prompt-submission-missing",
                    serde_json::json!({ "turnId": turn_id }),
                )
            })?;
    let claim = match expected_operation_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        Some(operation_id) => gold_band::acp::events::claim_session_turn_for_execution(
            &lifecycle_path,
            &turn_id,
            expected_revision,
            operation_id,
        )
        .map_err(command_error)?,
        None => AcpTurnExecutionClaim::Stale,
    };
    let claimed_owner = match claim {
        AcpTurnExecutionClaim::Claimed(owner) => owner,
        AcpTurnExecutionClaim::AlreadySettled(_) => return Ok(()),
        AcpTurnExecutionClaim::Stale => {
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
            return Err(CommandErrorVm::new(
                "conversation.prompt-execution-claim-lost",
                serde_json::json!({ "turnId": turn_id }),
            ));
        }
    };
    let claimed_revision = claimed_owner.revision;
    let claimed_operation_id = claimed_owner.operation_id.clone();
    let input = submission.input;
    let attachment_paths =
        (!submission.attachment_paths.is_empty()).then_some(submission.attachment_paths);
    let prompt_id = Some(turn_id.clone());
    let queued_dispatch = turn_id.starts_with(QUEUED_PROMPT_ID_PREFIX);
    let direct_mode = conversation_run_mode(&app, &locator.task_id)
        == Some(gold_band::config::ConversationRunMode::Direct);
    let agent_label = acp_turn_agent_label(&app, &locator);
    let project_id_for_emit = project_id.clone();
    let project_id_for_spawn = project_id_for_emit.clone();
    let task_id_for_emit = task_id.clone();
    let run_id_for_emit = run_id.clone();
    let round_id_for_emit = round_id.clone();
    let node_id_for_emit = node_id.clone();
    let attempt_id_for_emit = attempt_id.clone();
    let outer_node_id_for_emit = outer_node_id.clone();
    let outer_attempt_id_for_emit = outer_attempt_id.clone();
    let task_uuid_for_emit = app
        .run_status(&task_id, &run_id)
        .ok()
        .and_then(|run| run.task_uuid);
    let task_uuid_for_execution = task_uuid_for_emit.clone();
    let app_for_emit = app.clone_for_background();
    let app_handle_for_task = app_handle.clone();
    let lifecycle_path_for_stop = lifecycle_path.clone();
    let turn_id_for_stop = turn_id.clone();
    let execution = tauri::async_runtime::spawn_blocking(move || -> CommandResult<_> {
        let ConversationPromptInput {
            display_text,
            quotes,
        } = input;
        let prompt = conversation_prompt_text(&display_text, &quotes);
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
            let worker_ref = if worker_ref_path.exists() {
                Some(read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?)
            } else {
                None
            };
            let (session_mode, continue_ref) = existing_attempt_prompt_session_target(worker_ref);
            let prepared_prompt = app
                .prepare_dynamic_acp_prompt_for_attempt(
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
            let adapter_workspace_dir = prepared_prompt.adapter_workspace_dir;
            let session_workspace_dir = prepared_prompt.session_workspace_dir;
            let mut prompt_bundle = prepared_prompt.prompt;
            prompt_bundle.display_text = Some(display_text.clone());
            prompt_bundle.quotes = quotes.clone();
            if let Some(ref paths) = attachment_paths {
                if !paths.is_empty() {
                    let resolved = gold_band::provider::resolve_user_input_attachments(
                        paths,
                        &attempt_dir,
                        gold_band::provider::AttachmentProjectionPolicy::from(&app.config),
                    )
                    .map_err(command_error)?;
                    for attachment in resolved {
                        prompt_bundle.attachment_metas.push(attachment.meta);
                        prompt_bundle.content_blocks.push(attachment.block);
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
                task_uuid_for_execution.clone(),
                &run_id_for_live,
                &round_id_for_live,
                &node_id_for_live,
                &attempt_id_for_live,
                outer_node_id_for_live.clone(),
                outer_attempt_id_for_live.clone(),
            ));
            let prompt_accepted = app.acp_prompt_accepted_for(acp_live_event_context(
                &task_id_for_live,
                task_uuid_for_execution.clone(),
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
                adapter_workspace_dir,
                session_workspace_dir,
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
                claimed_owner.clone(),
                Some(&|event, timeline_position| {
                    live_update(
                        acp_live_event_context(
                            &task_id_for_live,
                            task_uuid_for_execution.clone(),
                            &run_id_for_live,
                            &round_id_for_live,
                            &node_id_for_live,
                            &attempt_id_for_live,
                            outer_node_id_for_live.clone(),
                            outer_attempt_id_for_live.clone(),
                        ),
                        event.clone(),
                        timeline_position,
                    )
                }),
                &app.acp_mcp_servers().unwrap_or_else(|error| {
                    warn!(
                        project_id = %app.paths.project_id,
                        %task_id,
                        %run_id,
                        %round_id,
                        %node_id,
                        %attempt_id,
                        %outer_node_id,
                        %outer_attempt_id,
                        provider,
                        %error,
                        "failed to load MCP servers for ACP session; continuing without MCP servers"
                    );
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
                    runtime_generation_owned: prompt_bundle.turn_control_mode
                        == TurnControlMode::RuntimeControlled,
                    lifecycle_file: Some(lifecycle_path_for_stop.clone()),
                    turn_id: Some(turn_id_for_stop.clone()),
                }),
            )
            .map_err(command_error)?;
            let outcome = acp_turn_outcome(&prompt_run);
            return Ok(outcome);
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
        let worker_ref = if worker_ref_path.exists() {
            Some(read_json::<WorkerRefState>(&worker_ref_path).map_err(command_error)?)
        } else {
            None
        };
        let (session_mode, continue_ref) = existing_attempt_prompt_session_target(worker_ref);
        let prepared_prompt = app
            .prepare_acp_prompt_for_attempt(
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
        let adapter_workspace_dir = prepared_prompt.adapter_workspace_dir;
        let session_workspace_dir = prepared_prompt.session_workspace_dir;
        let mut prompt_bundle = prepared_prompt.prompt;
        prompt_bundle.display_text = Some(display_text);
        prompt_bundle.quotes = quotes;
        if let Some(ref paths) = attachment_paths {
            if !paths.is_empty() {
                let resolved = gold_band::provider::resolve_user_input_attachments(
                    paths,
                    &attempt_dir,
                    gold_band::provider::AttachmentProjectionPolicy::from(&app.config),
                )
                .map_err(command_error)?;
                for attachment in resolved {
                    prompt_bundle.attachment_metas.push(attachment.meta);
                    prompt_bundle.content_blocks.push(attachment.block);
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
            task_uuid_for_execution.clone(),
            &run_id_for_live,
            &round_id_for_live,
            &node_id_for_live,
            &attempt_id_for_live,
            None,
            None,
        ));
        let prompt_accepted = app.acp_prompt_accepted_for(acp_live_event_context(
            &task_id_for_live,
            task_uuid_for_execution.clone(),
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
            adapter_workspace_dir,
            session_workspace_dir,
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
            claimed_owner.clone(),
            Some(&|event, timeline_position| {
                live_update(
                    acp_live_event_context(
                        &task_id_for_live,
                        task_uuid_for_execution.clone(),
                        &run_id_for_live,
                        &round_id_for_live,
                        &node_id_for_live,
                        &attempt_id_for_live,
                        None,
                        None,
                    ),
                    event.clone(),
                    timeline_position,
                )
            }),
            &app.acp_mcp_servers().unwrap_or_else(|error| {
                warn!(
                    project_id = %app.paths.project_id,
                    %task_id,
                    %run_id,
                    %round_id,
                    %node_id,
                    %attempt_id,
                    provider,
                    %error,
                    "failed to load MCP servers for ACP session; continuing without MCP servers"
                );
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
                runtime_generation_owned: prompt_bundle.turn_control_mode
                    == TurnControlMode::RuntimeControlled,
                lifecycle_file: Some(lifecycle_path_for_stop),
                turn_id: Some(turn_id_for_stop),
            }),
        )
        .map_err(command_error)?;
        let outcome = acp_turn_outcome(&prompt_run);
        Ok(outcome)
    })
    .await;
    let outcome = match execution {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = clear_auto_dispatch_reply_batch(&locator.attempt_dir(&app_for_emit));
            let settled = settle_failed_prompt_submission(
                &app_for_emit,
                &locator,
                &turn_id,
                Some(&claimed_operation_id),
                claimed_revision,
            );
            if settled {
                emit_acp_turn_finished(
                    &app_for_emit,
                    &locator,
                    &turn_id,
                    &agent_label,
                    AcpTurnOutcome::Failed,
                    AcpTurnBatchProgress::terminal(1),
                );
            }
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
                None,
            );
            return Err(error);
        }
        Err(_) => {
            let _ = clear_auto_dispatch_reply_batch(&locator.attempt_dir(&app_for_emit));
            let settled = settle_failed_prompt_submission(
                &app_for_emit,
                &locator,
                &turn_id,
                Some(&claimed_operation_id),
                claimed_revision,
            );
            if settled {
                emit_acp_turn_finished(
                    &app_for_emit,
                    &locator,
                    &turn_id,
                    &agent_label,
                    AcpTurnOutcome::Failed,
                    AcpTurnBatchProgress::terminal(1),
                );
            }
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
                None,
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
        None,
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
            task_uuid_for_emit,
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

    Ok(())
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
    let resume_cause = gold_band::app::observability::ResumeCause::PermissionResolved;
    app.record_metrics_resume_cause(&task_id, &run_id, resume_cause);
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
        if let Err(error) =
            write_acp_permission_response_signal(&attempt_dir, &request_id, option_id.clone())
        {
            app.clear_metrics_resume_cause(&task_id, &run_id, resume_cause);
            return Err(command_error(error));
        }
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
        if let Err(error) =
            write_acp_permission_response_signal(&attempt_dir, &request_id, option_id.clone())
        {
            app.clear_metrics_resume_cause(&task_id, &run_id, resume_cause);
            return Err(command_error(error));
        }
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

fn active_session_stop_is_idempotent_noop(
    has_acp_stop_owner: bool,
    runtime_was_active: bool,
    dynamic_resume_was_starting: bool,
) -> bool {
    !has_acp_stop_owner && !runtime_was_active && !dynamic_resume_was_starting
}

fn persist_active_session_stop(
    app: &gold_band::app::App,
    locator: &AttemptLocator,
    operation_id: &str,
) -> CommandResult<(Utf8PathBuf, Option<(String, String, u64)>, bool)> {
    let attempt_dir = locator.attempt_dir(app);
    let runtime_was_active = app
        .run_status(&locator.task_id, &locator.run_id)
        .is_ok_and(|run| run.status == RunStatus::Running && locator.matches_run_current(&run));
    let dynamic_resume_was_starting = match (locator.outer_node_id(), locator.outer_attempt_id()) {
        (Some(outer_node_id), Some(outer_attempt_id)) => app
            .dynamic_resume_target_is_active(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                outer_node_id,
                outer_attempt_id,
                &locator.node_id,
                &locator.attempt_id,
            )
            .map_err(command_error)?,
        _ => false,
    };
    // Persist the user intent before touching runtime-control files. A failure
    // in pause bookkeeping must not leave the provider turn running without a
    // durable cancellation request that recovery can observe.
    let lifecycle_path = acp_lifecycle_path(&attempt_dir);
    let stop = gold_band::acp::events::request_session_stop_outcome(
        &lifecycle_path,
        operation_id,
        &gold_band::acp::events::current_timestamp(),
    )
    .map_err(command_error)?;
    let stop_owner = stop
        .owner
        .map(|owner| (owner.turn_id, owner.operation_id, owner.revision));

    // A provider turn may not have reached durable admission yet while its
    // owning Runtime generation is already running. Runtime pause is the
    // authoritative durable stop in that startup window. Only a request with
    // neither an ACP owner nor a running Runtime owner is an idempotent no-op.
    if active_session_stop_is_idempotent_noop(
        stop_owner.is_some(),
        runtime_was_active,
        dynamic_resume_was_starting,
    ) {
        return Ok((attempt_dir, None, false));
    }

    let runtime_was_controlled = match attempt_is_runtime_controlled(app, locator) {
        Ok(value) => value,
        Err(error) => {
            warn!(?error, %attempt_dir, "failed to inspect ACP runtime control mode after durable stop");
            false
        }
    };
    if let Some((turn_id, operation_id, revision)) = stop_owner.as_ref() {
        let owner = gold_band::acp::events::AcpLifecycleOwner {
            turn_id: turn_id.clone(),
            operation_id: operation_id.clone(),
            revision: *revision,
        };
        if !gold_band::acp::events::lifecycle_owner_still_cancelling(&lifecycle_path, &owner)
            .map_err(command_error)?
        {
            return Ok((attempt_dir, None, runtime_was_active));
        }
        client::request_prompt_cancel(&attempt_dir);
        if !gold_band::acp::events::lifecycle_owner_still_cancelling(&lifecycle_path, &owner)
            .map_err(command_error)?
        {
            return Ok((attempt_dir, None, runtime_was_active));
        }
    }
    let pause_result = if let (Some(outer_node_id), Some(outer_attempt_id)) =
        (locator.outer_node_id(), locator.outer_attempt_id())
    {
        app.pause_dynamic_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            outer_node_id,
            outer_attempt_id,
            &locator.node_id,
            &locator.attempt_id,
            PauseReason::ProcessInterrupted,
        )
    } else {
        app.pause_attempt_runtime_state(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
            PauseReason::ProcessInterrupted,
        )
    };
    if let Err(error) = pause_result {
        if stop_owner.is_some() {
            // The ACP cancellation intent is already durable and remains the
            // accepted operation even if the Runtime projection cannot also
            // be paused. Provider cancellation still observes the owner.
            warn!(%error, %attempt_dir, "failed to pause runtime after durable ACP stop");
        } else {
            let runtime_is_still_active = app
                .run_status(&locator.task_id, &locator.run_id)
                .is_ok_and(|run| {
                    run.status == RunStatus::Running && locator.matches_run_current(&run)
                });
            if runtime_is_still_active {
                return Err(command_error(error));
            }
            // The owner settled while Stop was racing with startup. No
            // durable ACP intent was acquired and no Runtime generation was
            // paused, so this invocation is an idempotent no-op.
            return Ok((attempt_dir, None, false));
        }
    }
    if runtime_was_controlled {
        if let Err(error) = gold_band::acp::control::mark_runtime_interrupted(&attempt_dir) {
            warn!(%error, %attempt_dir, "failed to mark runtime interrupted after durable stop");
        }
    }
    Ok((attempt_dir, stop_owner, true))
}

fn spawn_active_session_stop_cleanup(
    app_handle: AppHandle,
    app: gold_band::app::App,
    project_id: Option<String>,
    locator: AttemptLocator,
    attempt_dir: Utf8PathBuf,
    stop_owner: Option<(String, String, u64)>,
) {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle_path = acp_lifecycle_path(&attempt_dir);
        let Some((turn_id, operation_id, revision)) = stop_owner.as_ref() else {
            return;
        };
        let owner = gold_band::acp::events::AcpLifecycleOwner {
            turn_id: turn_id.clone(),
            operation_id: operation_id.clone(),
            revision: *revision,
        };
        let owner_is_current =
            gold_band::acp::events::lifecycle_owner_still_cancelling(&lifecycle_path, &owner)
                .unwrap_or(false);
        if !owner_is_current {
            // The old turn may already be terminal and a newer turn may own
            // this attempt. Never send an attempt-wide cancel in that case.
            return;
        }
        match client::dispatch_attempt_prompt_cancel(&attempt_dir) {
            Ok(_) => {}
            Err(error) => {
                warn!(%error, %attempt_dir, "failed to dispatch accepted ACP stop request");
            }
        }
        // request_session_stop transferred lifecycle ownership away from the
        // provider runtime. The stop controller therefore owns terminal
        // settlement after dispatch; the old provider owner can only no-op.
        let decided_at = gold_band::acp::events::current_timestamp();
        let terminal_persisted = match gold_band::acp::events::persist_session_turn_terminal_owned(
            &lifecycle_path,
            turn_id,
            Some(operation_id),
            *revision,
            gold_band::acp::events::AcpLatestTurnStatus::Cancelled,
            "cancelled",
            &decided_at,
        ) {
            Ok(Some(_)) => {
                info!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    round_id = %locator.round_id,
                    node_id = %locator.node_id,
                    attempt_id = %locator.attempt_id,
                    outer_node_id = ?locator.outer_node_id,
                    outer_attempt_id = ?locator.outer_attempt_id,
                    %turn_id,
                    %operation_id,
                    outcome = "cancelled",
                    "conversation session stop reached terminal state"
                );
                true
            }
            Ok(None) => {
                tracing::debug!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    turn_id = %turn_id,
                    "stale conversation session stop settlement skipped"
                );
                false
            }
            Err(error) => {
                warn!(
                    project_id = %app.paths.project_id,
                    task_id = %locator.task_id,
                    run_id = %locator.run_id,
                    round_id = %locator.round_id,
                    node_id = %locator.node_id,
                    attempt_id = %locator.attempt_id,
                    %error,
                    %turn_id,
                    "failed to settle accepted ACP stop ownership"
                );
                false
            }
        };
        if terminal_persisted {
            touch_terminal_task_activity_best_effort(
                &app,
                &locator,
                Some(turn_id),
                "user-stop-terminal",
            );
        }
        if let Err(error) = client::settle_attempt_prompt_interactions(&attempt_dir) {
            warn!(%error, %attempt_dir, "failed to settle ACP interactions after accepted stop dispatch");
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
            return pending.identity.interaction_id;
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
    mut appearance: AppearancePreference,
    personalization: PersonalizationPreference,
    language: DesktopLanguage,
    use_local_claude: bool,
    verbose_logging: bool,
) -> CommandResult<PreferencesVm> {
    if appearance.schema_version != 2 {
        return Err(CommandErrorVm::new(
            "theme.contract-version-unsupported",
            serde_json::json!({ "schemaVersion": appearance.schema_version }),
        ));
    }
    let theme_catalog = gold_band::theme::builtin_theme_catalog().map_err(|error| {
        CommandErrorVm::new(error.code, serde_json::json!({ "detail": error.detail }))
    })?;
    if !theme_catalog
        .iter()
        .any(|theme| theme.id == appearance.theme_id)
    {
        return Err(CommandErrorVm::new(
            "theme.active-package-missing",
            serde_json::json!({ "themeId": appearance.theme_id }),
        ));
    }
    let mut personalization = normalize_personalization_preference(personalization)?;
    appearance.visual_quality_by_theme.retain(|theme_id, _| {
        theme_catalog.iter().any(|theme| {
            theme.id == *theme_id
                && theme
                    .capabilities
                    .contains(&gold_band::theme::ThemeCapability::VisualQualityProfiles)
        })
    });
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    match reconcile_wallpaper_personalization(&app.paths.user_gold_band_dir(), &mut personalization)
    {
        Ok(_) => {}
        Err(error) => warn!(
            error_code = error.code,
            "wallpaper personalization reconciliation skipped"
        ),
    }
    if context.config.use_local_claude != use_local_claude {
        ensure_no_active_acp_prompts_in_workspace(&app.paths.repo_root)?;
        gold_band::acp::client::close_workspace_connections_bounded(&app.paths.repo_root)
            .map_err(command_error)?;
    }
    let settings = app
        .set_user_desktop_preferences(
            appearance.clone(),
            personalization.clone(),
            language,
            use_local_claude,
            verbose_logging,
        )
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let log_level = settings.log_level.unwrap_or(context.config.log_level);
    set_runtime_log_level(log_level);
    Ok(preferences_vm(
        appearance,
        personalization.clone(),
        language,
        use_local_claude,
        log_level,
        load_resolved_avatar_preferences(&app.paths.user_gold_band_dir(), &personalization)
            .map_err(avatar_command_error)?,
        load_resolved_wallpaper_preferences(&app.paths.user_gold_band_dir()).unwrap_or_default(),
    ))
}

fn normalize_personalization_preference(
    mut preference: PersonalizationPreference,
) -> CommandResult<PersonalizationPreference> {
    if preference.schema_version != 4 {
        return Err(CommandErrorVm::new(
            "personalization.contract-version-unsupported",
            serde_json::json!({ "schemaVersion": preference.schema_version }),
        ));
    }
    for typography in [
        &mut preference.typography.ui,
        &mut preference.typography.editor,
    ] {
        if let FontStackPreference::Custom { families } = &mut typography.font_stack {
            if families.is_empty() || families.len() > MAX_FONT_STACK_FAMILIES {
                return Err(CommandErrorVm::new(
                    "personalization.font-stack-invalid",
                    serde_json::json!({ "count": families.len() }),
                ));
            }
            let mut seen = HashSet::new();
            for family in families.iter_mut() {
                *family = family.trim().to_string();
                if family.is_empty()
                    || family.chars().count() > MAX_FONT_FAMILY_CHARS
                    || family
                        .chars()
                        .any(|character| matches!(character, ',' | ';' | '{' | '}'))
                    || !seen.insert(family.to_lowercase())
                {
                    return Err(CommandErrorVm::new(
                        "personalization.font-stack-invalid",
                        serde_json::json!({ "count": families.len() }),
                    ));
                }
            }
        }
    }
    if let FontSizePreference::Custom { px } = &mut preference.typography.ui.font_size {
        *px = normalize_desktop_ui_font_size(*px);
    }
    if let FontSizePreference::Custom { px } = &mut preference.typography.editor.font_size {
        *px = normalize_desktop_editor_font_size(*px);
    }
    for avatar in [&preference.avatars.agent, &preference.avatars.user] {
        if matches!(&avatar.image, AvatarPreference::User { asset_id } if asset_id.trim().is_empty())
        {
            return Err(CommandErrorVm::new(
                "personalization.avatar-invalid",
                serde_json::json!({}),
            ));
        }
    }
    for wallpaper in [
        &preference.wallpaper.by_color_scheme.light,
        &preference.wallpaper.by_color_scheme.dark,
    ] {
        if matches!(&wallpaper.image, WallpaperImagePreference::User { asset_id } if asset_id.trim().is_empty())
        {
            return Err(CommandErrorVm::new(
                "personalization.wallpaper-invalid",
                serde_json::json!({}),
            ));
        }
        if !(MIN_DESKTOP_WALLPAPER_OPACITY_PERCENT..=MAX_DESKTOP_WALLPAPER_OPACITY_PERCENT)
            .contains(&wallpaper.opacity_percent)
        {
            return Err(CommandErrorVm::new(
                "personalization.wallpaper-opacity-invalid",
                serde_json::json!({
                    "min": MIN_DESKTOP_WALLPAPER_OPACITY_PERCENT,
                    "max": MAX_DESKTOP_WALLPAPER_OPACITY_PERCENT,
                }),
            ));
        }
    }
    Ok(preference)
}

#[tauri::command]
pub fn save_desktop_avatar(
    state: State<'_, DesktopState>,
    input: SaveDesktopAvatarInput,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let kind = input.kind;
    let shape = input.shape;
    let avatars =
        save_avatar_image(&app.paths.user_gold_band_dir(), input).map_err(avatar_command_error)?;
    let profile = avatar_profile(&avatars, kind);
    let asset_id = profile.selected_avatar_id.clone().ok_or_else(|| {
        CommandErrorVm::new("personalization.avatar-invalid", serde_json::json!({}))
    })?;
    let mut personalization = context.config.personalization.clone();
    let target = avatar_personalization_mut(&mut personalization, kind);
    target.image = AvatarPreference::User { asset_id };
    target.shape = AvatarShapePreference::Custom {
        value: personalization_avatar_shape(shape),
    };
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub fn select_recent_desktop_avatar(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
    avatar_id: String,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    select_recent_avatar(&app.paths.user_gold_band_dir(), kind, &avatar_id)
        .map_err(avatar_command_error)?;
    let mut personalization = context.config.personalization.clone();
    avatar_personalization_mut(&mut personalization, kind).image = AvatarPreference::User {
        asset_id: avatar_id,
    };
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub fn save_desktop_avatar_shape(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
    shape: Option<AvatarShape>,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    if let Some(shape) = shape {
        save_avatar_shape(&app.paths.user_gold_band_dir(), kind, shape)
            .map_err(avatar_command_error)?;
    }
    let mut personalization = context.config.personalization.clone();
    avatar_personalization_mut(&mut personalization, kind).shape =
        shape.map_or(AvatarShapePreference::Theme, |value| {
            AvatarShapePreference::Custom {
                value: personalization_avatar_shape(value),
            }
        });
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub fn clear_desktop_avatar(
    state: State<'_, DesktopState>,
    kind: AvatarKind,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    clear_avatar(&app.paths.user_gold_band_dir(), kind).map_err(avatar_command_error)?;
    let mut personalization = context.config.personalization.clone();
    avatar_personalization_mut(&mut personalization, kind).image = AvatarPreference::Theme;
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub async fn import_desktop_wallpaper(
    state: State<'_, DesktopState>,
    input: ImportDesktopWallpaperInput,
) -> CommandResult<PreferencesVm> {
    let color_scheme = input.color_scheme;
    let initial_context = state.context().map_err(command_error)?;
    let root = initial_context.app().paths.user_gold_band_dir();
    let retained_asset_ids = [
        &initial_context
            .config
            .personalization
            .wallpaper
            .by_color_scheme
            .light
            .image,
        &initial_context
            .config
            .personalization
            .wallpaper
            .by_color_scheme
            .dark
            .image,
    ]
    .into_iter()
    .filter_map(|image| match image {
        WallpaperImagePreference::User { asset_id } => Some(asset_id.clone()),
        WallpaperImagePreference::Theme => None,
    })
    .collect::<HashSet<_>>();
    let saved = spawn_blocking_command(move || {
        import_wallpaper_image(&root, input, &retained_asset_ids).map_err(wallpaper_command_error)
    })
    .await?;
    // Image processing runs off-thread; merge into the latest preference
    // snapshot so an unrelated settings change cannot be overwritten.
    let context = state.context().map_err(command_error)?;
    let mut personalization = context.config.personalization.clone();
    personalization
        .wallpaper
        .for_color_scheme_mut(color_scheme)
        .image = WallpaperImagePreference::User {
        asset_id: saved.asset_id,
    };
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub async fn select_recent_desktop_wallpaper(
    state: State<'_, DesktopState>,
    input: SelectRecentDesktopWallpaperInput,
) -> CommandResult<PreferencesVm> {
    let root = state
        .context()
        .map_err(command_error)?
        .app()
        .paths
        .user_gold_band_dir();
    let selected_id = input.wallpaper_id.clone();
    spawn_blocking_command(move || {
        select_recent_wallpaper(&root, &selected_id).map_err(wallpaper_command_error)
    })
    .await?;
    let context = state.context().map_err(command_error)?;
    let mut personalization = context.config.personalization.clone();
    personalization
        .wallpaper
        .for_color_scheme_mut(input.color_scheme)
        .image = WallpaperImagePreference::User {
        asset_id: input.wallpaper_id,
    };
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub fn save_desktop_wallpaper_opacity(
    state: State<'_, DesktopState>,
    input: SaveDesktopWallpaperOpacityInput,
) -> CommandResult<PreferencesVm> {
    if !(MIN_DESKTOP_WALLPAPER_OPACITY_PERCENT..=MAX_DESKTOP_WALLPAPER_OPACITY_PERCENT)
        .contains(&input.opacity_percent)
    {
        return Err(CommandErrorVm::new(
            "personalization.wallpaper-opacity-invalid",
            serde_json::json!({
                "min": MIN_DESKTOP_WALLPAPER_OPACITY_PERCENT,
                "max": MAX_DESKTOP_WALLPAPER_OPACITY_PERCENT,
            }),
        ));
    }
    let context = state.context().map_err(command_error)?;
    let mut personalization = context.config.personalization.clone();
    personalization
        .wallpaper
        .for_color_scheme_mut(input.color_scheme)
        .opacity_percent = input.opacity_percent;
    persist_desktop_personalization(&state, &context, personalization)
}

#[tauri::command]
pub fn restore_theme_desktop_wallpaper(
    state: State<'_, DesktopState>,
    input: RestoreThemeDesktopWallpaperInput,
) -> CommandResult<PreferencesVm> {
    let context = state.context().map_err(command_error)?;
    let mut personalization = context.config.personalization.clone();
    personalization
        .wallpaper
        .for_color_scheme_mut(input.color_scheme)
        .image = WallpaperImagePreference::Theme;
    persist_desktop_personalization(&state, &context, personalization)
}

fn avatar_profile(
    avatars: &AvatarPreferencesVm,
    kind: AvatarKind,
) -> &crate::avatar::AvatarProfileVm {
    match kind {
        AvatarKind::Agent => &avatars.agent,
        AvatarKind::User => &avatars.user,
    }
}

fn avatar_personalization_mut(
    personalization: &mut PersonalizationPreference,
    kind: AvatarKind,
) -> &mut gold_band::config::AvatarPersonalization {
    match kind {
        AvatarKind::Agent => &mut personalization.avatars.agent,
        AvatarKind::User => &mut personalization.avatars.user,
    }
}

fn personalization_avatar_shape(shape: AvatarShape) -> PersonalizationAvatarShape {
    match shape {
        AvatarShape::Circle => PersonalizationAvatarShape::Circle,
        AvatarShape::Square => PersonalizationAvatarShape::Square,
    }
}

fn persist_desktop_personalization(
    state: &DesktopState,
    context: &crate::state::DesktopContext,
    personalization: PersonalizationPreference,
) -> CommandResult<PreferencesVm> {
    let app = context.app();
    let settings = app
        .set_user_desktop_personalization(personalization.clone())
        .map_err(command_error)?;
    state
        .update_settings_config(&settings)
        .map_err(command_error)?;
    let avatars =
        load_resolved_avatar_preferences(&app.paths.user_gold_band_dir(), &personalization)
            .map_err(avatar_command_error)?;
    let wallpapers =
        load_resolved_wallpaper_preferences(&app.paths.user_gold_band_dir()).unwrap_or_default();
    Ok(preferences_vm(
        context.config.appearance.clone(),
        personalization,
        context.config.desktop_language,
        context.config.use_local_claude,
        context.config.log_level,
        avatars,
        wallpapers,
    ))
}

fn avatar_command_error(error: crate::avatar::AvatarError) -> CommandErrorVm {
    CommandErrorVm::new(error.code, error.params)
}

fn wallpaper_command_error(error: crate::wallpaper::WallpaperError) -> CommandErrorVm {
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
    if let Some(error) = error.downcast_ref::<gold_band::app::RuntimeRecoveryError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    if let Some(error) = error.downcast_ref::<gold_band::storage::core_state::CoreStateError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    if let Some(error) = error.downcast_ref::<gold_band::storage::ProjectManifestError>() {
        return CommandErrorVm::new(error.code(), error.params());
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
    if let Some(error) = error.downcast_ref::<gold_band::app::WorkflowTemplateCommandError>() {
        return CommandErrorVm::new(error.code(), error.params());
    }
    if let Some(error) =
        error.downcast_ref::<gold_band::workflow_model_binding::WorkflowModelBindingError>()
    {
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
        PromptQueueError::RevisionConflict => "conversation.prompt-queue-revision-conflict",
        PromptQueueError::InvalidOrder => "conversation.prompt-queue-invalid-order",
        PromptQueueError::Storage => "conversation.prompt-queue-storage-failed",
    };
    CommandErrorVm::new(code, serde_json::json!({}))
}

fn validate_conversation_prompt_input(
    input: &ConversationPromptInput,
    attachment_paths: Option<&[String]>,
) -> CommandResult<()> {
    let attachment_paths = attachment_paths.unwrap_or_default();
    if input.display_text.trim().is_empty() && attachment_paths.is_empty() {
        return Err(CommandErrorVm::new(
            "conversation.prompt-empty",
            serde_json::json!({}),
        ));
    }
    if let Some(code) = crate::view_models_conversation::validate_attachment_paths(attachment_paths)
        .into_iter()
        .next()
    {
        return Err(CommandErrorVm::new(code, serde_json::json!({})));
    }
    if input.quotes.len() > MAX_USER_PROMPT_QUOTES {
        return Err(CommandErrorVm::new(
            "conversation.prompt-quote-count-exceeded",
            serde_json::json!({ "maxQuotes": MAX_USER_PROMPT_QUOTES }),
        ));
    }
    let mut quote_ids = HashSet::with_capacity(input.quotes.len());
    let mut quote_chars = 0usize;
    for quote in &input.quotes {
        if quote.id.trim().is_empty()
            || quote.source_message_key.trim().is_empty()
            || quote.text.trim().is_empty()
            || !quote_ids.insert(quote.id.as_str())
        {
            return Err(CommandErrorVm::new(
                "conversation.prompt-quote-invalid",
                serde_json::json!({}),
            ));
        }
        if quote.id.len() > MAX_USER_PROMPT_QUOTE_ID_BYTES
            || quote.source_message_key.len() > MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES
        {
            return Err(CommandErrorVm::new(
                "conversation.prompt-quote-metadata-too-long",
                serde_json::json!({
                    "maxIdBytes": MAX_USER_PROMPT_QUOTE_ID_BYTES,
                    "maxSourceKeyBytes": MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES,
                }),
            ));
        }
        let remaining = MAX_USER_PROMPT_QUOTE_CHARS.saturating_sub(quote_chars);
        let chars = quote.text.chars().take(remaining + 1).count();
        quote_chars += chars;
        if quote_chars > MAX_USER_PROMPT_QUOTE_CHARS {
            return Err(CommandErrorVm::new(
                "conversation.prompt-quote-limit-exceeded",
                serde_json::json!({ "maxChars": MAX_USER_PROMPT_QUOTE_CHARS }),
            ));
        }
    }
    Ok(())
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
    gold_band::acp::events::load_session_metadata_value(&path, None)
        .ok()
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
    gold_band::acp::events::read_attempt_session_model(&attempt_dir.join("acp.session.json"))
}

fn current_acp_session_model_name(attempt_dir: &Utf8PathBuf) -> Option<String> {
    gold_band::acp::events::read_attempt_session_model_name(&attempt_dir.join("acp.session.json"))
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
    gold_band::acp::events::load_session_metadata_value(&path, None)
        .ok()
        .and_then(|value| value.get("configOptionOverrides").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
struct AcpCatalogSelectOption {
    category: String,
    values: BTreeSet<String>,
}

#[derive(Debug, Clone, Default)]
struct AcpSessionConfigCatalog {
    observed_at: Option<String>,
    models: Option<BTreeSet<String>>,
    modes: Option<BTreeSet<String>>,
    config_options: Option<BTreeMap<String, AcpCatalogSelectOption>>,
}

impl AcpSessionConfigCatalog {
    fn from_value(value: &serde_json::Value, observed_at: Option<String>) -> Self {
        let select_options = select_config_options_from_capabilities(Some(value));
        let config_options = value.get("configOptions").map(|_| {
            select_options
                .iter()
                .map(|option| {
                    (
                        option.id.clone(),
                        AcpCatalogSelectOption {
                            category: option.category.clone().unwrap_or_else(|| option.id.clone()),
                            values: option
                                .options
                                .iter()
                                .map(|value| value.value.clone())
                                .collect(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>()
        });
        let models = select_options
            .iter()
            .any(|option| option.category.as_deref() == Some("model"))
            .then(|| {
                supported_models_from_capabilities(Some(value))
                    .into_iter()
                    .map(|option| option.id)
                    .collect()
            })
            .or_else(|| {
                value
                    .get("models")
                    .map(|models| acp_catalog_grouped_ids(models, "availableModels", true))
            });
        let modes = select_options
            .iter()
            .any(|option| option.id == "mode" || option.category.as_deref() == Some("mode"))
            .then(|| {
                supported_modes_from_capabilities(Some(value))
                    .into_iter()
                    .map(|option| option.id)
                    .collect()
            })
            .or_else(|| {
                value
                    .get("modes")
                    .map(|modes| acp_catalog_grouped_ids(modes, "availableModes", false))
            });
        Self {
            observed_at,
            models,
            modes,
            config_options,
        }
    }

    fn supports_model(&self, value: &str) -> Option<bool> {
        self.models.as_ref().map(|values| values.contains(value))
    }

    fn supports_mode(&self, value: &str) -> Option<bool> {
        self.modes.as_ref().map(|values| values.contains(value))
    }

    fn supports_config_value(&self, option_id: &str, value: &str) -> Option<bool> {
        self.config_options.as_ref().map(|options| {
            options
                .get(option_id)
                .is_some_and(|option| option.values.contains(value))
        })
    }
}

#[derive(Debug, Clone)]
struct AcpSessionConfigCatalogContext {
    session: AcpSessionConfigCatalog,
    newer_doctor: Option<AcpSessionConfigCatalog>,
}

impl AcpSessionConfigCatalogContext {
    fn effective(&self) -> &AcpSessionConfigCatalog {
        self.newer_doctor.as_ref().unwrap_or(&self.session)
    }
}

fn acp_catalog_grouped_ids(
    value: &serde_json::Value,
    list_key: &str,
    model: bool,
) -> BTreeSet<String> {
    value
        .get(list_key)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let key = if model { "modelId" } else { "id" };
            item.get(key)
                .or_else(|| item.get("id"))
                .or_else(|| item.get("value"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn acp_catalog_timestamp(value: &str) -> Option<i64> {
    value
        .trim()
        .trim_end_matches('Z')
        .parse::<i64>()
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(value.trim())
                .ok()
                .map(|value| value.timestamp())
        })
}

fn acp_catalog_observation_is_newer(candidate: &str, current: Option<&str>) -> bool {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return false;
    }
    let Some(current) = current.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    if candidate == current {
        return false;
    }
    match (
        acp_catalog_timestamp(candidate),
        acp_catalog_timestamp(current),
    ) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => candidate > current,
    }
}

fn acp_session_config_catalog_context(
    app: &App,
    locator: &AttemptLocator,
    session: &serde_json::Value,
) -> AcpSessionConfigCatalogContext {
    let session_catalog = AcpSessionConfigCatalog::from_value(
        session,
        session
            .get("configCatalogObservedAt")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    );
    let newer_doctor = acp_turn_provider_id(app, locator)
        .and_then(|provider| app.provider_diagnostics().remove(&provider))
        .filter(|diagnostic| diagnostic.available)
        .and_then(|diagnostic| {
            let capabilities = diagnostic.capabilities?;
            acp_catalog_observation_is_newer(
                &diagnostic.checked_at,
                session_catalog.observed_at.as_deref(),
            )
            .then(|| {
                AcpSessionConfigCatalog::from_value(&capabilities, Some(diagnostic.checked_at))
            })
        });
    AcpSessionConfigCatalogContext {
        session: session_catalog,
        newer_doctor,
    }
}

fn acp_session_config_value_unavailable(
    category: &str,
    config_id: &str,
    value: &str,
    available_values: impl IntoIterator<Item = String>,
) -> CommandErrorVm {
    CommandErrorVm::new(
        gold_band::acp::client::ACP_SESSION_CONFIG_VALUE_UNAVAILABLE_CODE,
        serde_json::json!({
            "category": category,
            "configId": config_id,
            "value": value,
            "availableValues": available_values.into_iter().collect::<Vec<_>>(),
        }),
    )
}

fn validate_acp_catalog_model(catalog: &AcpSessionConfigCatalog, value: &str) -> CommandResult<()> {
    if catalog.supports_model(value) == Some(false) {
        return Err(acp_session_config_value_unavailable(
            "model",
            "model",
            value,
            catalog.models.clone().unwrap_or_default(),
        ));
    }
    Ok(())
}

fn validate_acp_catalog_mode(catalog: &AcpSessionConfigCatalog, value: &str) -> CommandResult<()> {
    if catalog.supports_mode(value) == Some(false) {
        return Err(acp_session_config_value_unavailable(
            "mode",
            "mode",
            value,
            catalog.modes.clone().unwrap_or_default(),
        ));
    }
    Ok(())
}

fn validate_acp_catalog_config_value(
    catalog: &AcpSessionConfigCatalog,
    option_id: &str,
    value: &str,
) -> CommandResult<()> {
    let option = catalog
        .config_options
        .as_ref()
        .and_then(|options| options.get(option_id));
    if option.is_some_and(|option| option.values.contains(value)) {
        return Ok(());
    }
    Err(acp_session_config_value_unavailable(
        option
            .map(|option| option.category.as_str())
            .unwrap_or("config"),
        option_id,
        value,
        option
            .map(|option| option.values.clone())
            .unwrap_or_default(),
    ))
}

fn apply_acp_catalog_refresh_marker(
    session: &mut serde_json::Value,
    catalogs: &AcpSessionConfigCatalogContext,
) {
    let Some(doctor) = catalogs.newer_doctor.as_ref() else {
        return;
    };
    let model_requires_refresh = session
        .get("modelOverride")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| {
            doctor.supports_model(value) == Some(true)
                && catalogs.session.supports_model(value) != Some(true)
        });
    let mode_requires_refresh = session
        .get("permissionModeOverride")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| {
            doctor.supports_mode(value) == Some(true)
                && catalogs.session.supports_mode(value) != Some(true)
        });
    let option_requires_refresh = session
        .get("configOptionOverrides")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|overrides| {
            overrides.iter().any(|(option_id, value)| {
                value.as_str().is_some_and(|value| {
                    doctor.supports_config_value(option_id, value) == Some(true)
                        && catalogs.session.supports_config_value(option_id, value) != Some(true)
                })
            })
        });
    let Some(object) = session.as_object_mut() else {
        return;
    };
    if model_requires_refresh || mode_requires_refresh || option_requires_refresh {
        if let Some(observed_at) = doctor.observed_at.as_ref() {
            object.insert(
                "configCatalogRefreshRequiredAt".to_string(),
                serde_json::Value::String(observed_at.clone()),
            );
        }
    } else {
        object.remove("configCatalogRefreshRequiredAt");
    }
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
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
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

    let metadata = load_session_metadata(&path, None).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value = serde_json::to_value(metadata).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let catalogs = acp_session_config_catalog_context(&app, &locator, &value);
    if let Some(model_id) = model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validate_acp_catalog_model(catalogs.effective(), model_id)?;
    }

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
    apply_acp_catalog_refresh_marker(&mut value, &catalogs);
    value = gold_band::acp::events::patch_session_metadata(&path, |current| {
        if let Some(session) = current.as_object_mut() {
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
            if let Some(models) = current
                .get_mut("models")
                .and_then(|models| models.as_object_mut())
            {
                models.insert(
                    "currentModelId".to_string(),
                    serde_json::Value::String(model_id.to_string()),
                );
            }
            set_acp_config_option_current_value(current, "model", model_id);
        }
        apply_acp_catalog_refresh_marker(current, &catalogs);
        Ok(())
    })
    .map_err(|error| {
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
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
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

    let metadata = load_session_metadata(&path, None).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value = serde_json::to_value(metadata).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let catalogs = acp_session_config_catalog_context(&app, &locator, &value);
    if let Some(permission_mode_id) = permission_mode_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validate_acp_catalog_mode(catalogs.effective(), permission_mode_id)?;
    }

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
    apply_acp_catalog_refresh_marker(&mut value, &catalogs);
    value = gold_band::acp::events::patch_session_metadata(&path, |current| {
        if let Some(session) = current.as_object_mut() {
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
            if let Some(modes) = current
                .get_mut("modes")
                .and_then(|modes| modes.as_object_mut())
            {
                modes.insert(
                    "currentModeId".to_string(),
                    serde_json::Value::String(permission_mode_id.to_string()),
                );
            }
            set_acp_config_option_current_value(current, "mode", permission_mode_id);
        }
        apply_acp_catalog_refresh_marker(current, &catalogs);
        Ok(())
    })
    .map_err(|error| {
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
    let locator = AttemptLocator::new(
        task_id.clone(),
        run_id.clone(),
        round_id.clone(),
        node_id.clone(),
        attempt_id.clone(),
        outer_node_id.clone(),
        outer_attempt_id.clone(),
    );
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
    let metadata = load_session_metadata(&path, None).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-read-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let mut value = serde_json::to_value(metadata).map_err(|error| {
        CommandErrorVm::new(
            "acp.session-parse-error",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    let option_id = option_id.trim();
    let catalogs = acp_session_config_catalog_context(&app, &locator, &value);
    let normalized_value = option_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(selected) = normalized_value {
        validate_acp_catalog_config_value(catalogs.effective(), option_id, selected)?;
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
    apply_acp_catalog_refresh_marker(&mut value, &catalogs);
    value = gold_band::acp::events::patch_session_metadata(&path, |current| {
        if let Some(session) = current.as_object_mut() {
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
            set_acp_config_option_current_value(current, option_id, selected);
        }
        apply_acp_catalog_refresh_marker(current, &catalogs);
        Ok(())
    })
    .map_err(|error| {
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
    let _ = state.record_heartbeat_activity();
    let app = resolve_command_app(state.inner(), project_id.as_deref())?;

    // Reclaim the durable scheduled occurrence before writing the response file.
    // The ACP waiter may resume immediately after the file is visible.
    resume_scheduled_interaction(
        state.inner(),
        &app,
        &task_id,
        &run_id,
        &round_id,
        &attempt_id,
    )
    .await?;

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
    let resume_cause = gold_band::app::observability::ResumeCause::ElicitationResolved;
    app.record_metrics_resume_cause(&task_id, &run_id, resume_cause);
    if let Err(error) = write_elicitation_response(
        &attempt_dir,
        &elicitation_id,
        action.clone(),
        content.clone(),
        current_timestamp(),
    ) {
        app.clear_metrics_resume_cause(&task_id, &run_id, resume_cause);
        return Err(command_error(error));
    }

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

fn scheduled_attention_requires_coordinator(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    attempt_id: &str,
) -> crate::scheduled_service::ScheduledServiceResult<bool> {
    let database_path = app.paths.scheduler_db_path();
    if !database_path.exists() {
        return Ok(false);
    }
    let database = gold_band::scheduler::db::ScheduledTaskDatabase::open(database_path)
        .map_err(crate::scheduled_service::ScheduledServiceError::from_database)?;
    Ok(database
        .find_attention_occurrence_by_links(
            &app.paths.project_id,
            task_id,
            run_id,
            round_id,
            attempt_id,
        )
        .map_err(crate::scheduled_service::ScheduledServiceError::from_database)?
        .is_some())
}

async fn resume_scheduled_interaction(
    state: &DesktopState,
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    attempt_id: &str,
) -> CommandResult<Option<String>> {
    let requires_coordinator =
        scheduled_attention_requires_coordinator(app, task_id, run_id, round_id, attempt_id)
            .map_err(|error| CommandErrorVm::new(error.code.to_string(), error.params))?;
    if !requires_coordinator {
        return Ok(None);
    }

    let coordinator = state.scheduler_coordinator().map_err(|_| {
        CommandErrorVm::new(
            gold_band::scheduler::occurrence::ScheduledErrorCode::CoordinatorUnavailable
                .to_string(),
            serde_json::json!({ "operation": "resume-attention" }),
        )
    })?;
    coordinator
        .resume_attention(
            app.paths.repo_root.clone(),
            task_id.to_string(),
            run_id.to_string(),
            round_id.to_string(),
            attempt_id.to_string(),
        )
        .await
        .map_err(|error| CommandErrorVm::new(error.code.to_string(), error.params))?
        .ok_or_else(|| {
            CommandErrorVm::new(
                gold_band::scheduler::occurrence::ScheduledErrorCode::NotFound.to_string(),
                serde_json::json!({
                    "operation": "resume-attention",
                    "taskId": task_id,
                    "runId": run_id,
                    "roundId": round_id,
                    "attemptId": attempt_id,
                }),
            )
        })
        .map(Some)
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
    use gold_band::app::{WorkflowTemplate, WorkflowTemplateStore};
    use gold_band::config::{ConversationWorkspaceEntry, ResolvedColorScheme, RuntimeConfig};
    use gold_band::dsl::{NodeDsl, WorkerNode, WorkflowDsl};
    use gold_band::dynamic::DynamicGraphState;
    use gold_band::runtime::RoundState;
    use gold_band::runtime::TaskState;
    use gold_band::scheduler::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use gold_band::storage::write_json;
    use gold_band::workflow_model_binding::{
        TaskAuthoringWorkflow, WorkerModelBinding, WorkflowModelBindings,
    };
    use std::collections::BTreeMap;

    fn frontend_error_input(message: String) -> FrontendErrorReportInput {
        FrontendErrorReportInput {
            kind: FrontendErrorKindInput::ReactUncaught,
            message,
            stack: Some("stack".to_string()),
            component_stack: Some("component-stack".to_string()),
            source: Some("main.tsx".to_string()),
            line: Some(12),
            column: Some(8),
            active_element: Some("button#send".to_string()),
            last_pointer_target: Some("button#send".to_string()),
            last_pointer_at: Some("2026-08-25T10:00:00Z".to_string()),
            pathname: Some("/conversation/task-001".to_string()),
            user_agent: Some("GoldBandWebView".to_string()),
        }
    }

    #[test]
    fn frontend_error_report_normalization_enforces_unicode_safe_bounds() {
        let mut input = frontend_error_input("错".repeat(FRONTEND_ERROR_MESSAGE_MAX_CHARS + 5));
        input.source = Some("app://localhost/main.js?token=secret".to_string());
        let report = input.normalize();

        assert_eq!(
            report.message.chars().count(),
            FRONTEND_ERROR_MESSAGE_MAX_CHARS
        );
        assert_eq!(
            report.message,
            "错".repeat(FRONTEND_ERROR_MESSAGE_MAX_CHARS)
        );
        assert_eq!(report.kind, FrontendErrorKindInput::ReactUncaught);
        assert_eq!(report.line, Some(12));
        assert_eq!(report.source.as_deref(), Some("app://localhost/main.js"));
    }

    #[test]
    fn frontend_error_command_accepts_bounded_structured_report() {
        assert!(report_frontend_error(frontend_error_input("render failed".to_string())).is_ok());
    }

    fn webview_environment_input(user_agent: String) -> WebviewEnvironmentReportInput {
        WebviewEnvironmentReportInput {
            user_agent,
            capabilities: WebviewCapabilitiesInput {
                regexp_lookbehind: false,
                css_color_mix: false,
                css_container_queries: false,
                css_has_selector: true,
                css_backdrop_filter: true,
                css_oklch: true,
                css_grid: true,
                css_custom_properties: true,
                resize_observer: true,
                structured_clone: true,
                web_assembly: true,
            },
            policy: WebviewFeaturePolicyInput {
                tier: WebviewSupportTierInput::Compatible,
                theme_rendering: WebviewThemeRenderingInput::FallbackTokens,
                responsive_layout: WebviewResponsiveLayoutInput::Measured,
                code_highlighting: WebviewCodeHighlightingInput::Wasm,
                visual_material: WebviewVisualMaterialInput::Solid,
            },
        }
    }

    #[test]
    fn webview_environment_report_normalization_enforces_user_agent_bound() {
        let report =
            webview_environment_input("W".repeat(WEBVIEW_USER_AGENT_MAX_CHARS + 7)).normalize();
        assert_eq!(
            report.user_agent.chars().count(),
            WEBVIEW_USER_AGENT_MAX_CHARS
        );
        assert_eq!(report.policy.tier, WebviewSupportTierInput::Compatible);
    }

    #[test]
    fn webview_environment_policy_rejects_unknown_values() {
        let result = serde_json::from_value::<WebviewFeaturePolicyInput>(serde_json::json!({
            "tier": "compatible",
            "themeRendering": "legacy-css",
            "responsiveLayout": "measured",
            "codeHighlighting": "wasm",
            "visualMaterial": "solid"
        }));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn webview_environment_command_returns_platform_facts() {
        let facts =
            report_webview_environment(webview_environment_input("GoldBandWebView".to_string()))
                .await
                .expect("webview facts");
        assert_eq!(facts.platform, std::env::consts::OS);
        assert_eq!(facts.architecture, std::env::consts::ARCH);
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(facts.os_version, None);
            assert_eq!(facts.webkit_bundle_version, None);
        }
    }

    fn direct_run_completed(outcome: RunOutcome) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunCompleted {
            event_id: "run-event-001".to_string(),
            occurred_at: "2026-08-18T10:00:00Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: Some("task-uuid-001".to_string()),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_label: "Claude".to_string(),
            outcome,
            task_title: None,
            completion_agent_label: Some("Claude".to_string()),
        }
    }

    fn acp_turn_finished(outcome: AcpTurnOutcome, continues: bool) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::AcpTurnFinished {
            event_id: "turn-event-001".to_string(),
            occurred_at: "2026-08-18T10:00:01Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-001".to_string(),
            task_id: "task-001".to_string(),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            turn_id: "turn-001".to_string(),
            agent_label: "Claude".to_string(),
            outcome,
            batch_progress: AcpTurnBatchProgress {
                completed_reply_count: 1,
                continues,
            },
            task_title: None,
        }
    }

    #[test]
    fn prompt_submission_started_response_exposes_durable_turn_identity() {
        let response = ConversationPromptSubmitVm {
            kind: "acp-session-started".to_string(),
            turn_id: Some("turn-001".to_string()),
            revision: Some(7),
            operation_id: Some("prompt:operation-001".to_string()),
            session: None,
            run: None,
            lifecycle: None,
            admission_was_terminal: false,
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["kind"], "acp-session-started");
        assert_eq!(value["turnId"], "turn-001");
        assert_eq!(value["revision"], 7);
        assert_eq!(value["operationId"], "prompt:operation-001");
        assert!(value["session"].is_null());
        assert!(value.get("admissionWasTerminal").is_none());
    }

    #[test]
    fn direct_terminal_result_projection_waits_for_the_terminal_reply_batch() {
        assert!(
            conversation_terminal_result_candidate(&direct_run_completed(RunOutcome::Success))
                .is_none()
        );
        assert!(
            conversation_terminal_result_candidate(&acp_turn_finished(
                AcpTurnOutcome::Completed,
                true
            ))
            .is_none()
        );

        let completed = conversation_terminal_result_candidate(&acp_turn_finished(
            AcpTurnOutcome::Completed,
            false,
        ))
        .unwrap();
        assert_eq!(
            completed.result.kind,
            ConversationTerminalResultKind::Completed
        );
        assert!(completed.requires_direct_mode_check);
    }

    #[test]
    fn direct_terminal_result_projection_maps_failure_and_stop_semantics() {
        let failed =
            conversation_terminal_result_candidate(&direct_run_completed(RunOutcome::Failure))
                .unwrap();
        assert_eq!(failed.result.kind, ConversationTerminalResultKind::Failed);
        assert!(!failed.requires_direct_mode_check);

        let stopped = conversation_terminal_result_candidate(&acp_turn_finished(
            AcpTurnOutcome::Cancelled,
            false,
        ))
        .unwrap();
        assert_eq!(stopped.result.kind, ConversationTerminalResultKind::Stopped);
    }

    #[test]
    fn doctor_catalog_newer_than_session_marks_the_selected_override_for_refresh() {
        let mut session = serde_json::json!({
            "configCatalogObservedAt": "100Z",
            "modelOverride": "new-model",
            "configOptions": [{
                "id": "model",
                "category": "model",
                "type": "select",
                "options": [{ "value": "old-model" }]
            }]
        });
        let catalogs = AcpSessionConfigCatalogContext {
            session: AcpSessionConfigCatalog::from_value(&session, Some("100Z".to_string())),
            newer_doctor: Some(AcpSessionConfigCatalog::from_value(
                &serde_json::json!({
                    "configOptions": [{
                        "id": "model",
                        "category": "model",
                        "type": "select",
                        "options": [{ "value": "new-model" }]
                    }]
                }),
                Some("200Z".to_string()),
            )),
        };

        validate_acp_catalog_model(catalogs.effective(), "new-model").unwrap();
        apply_acp_catalog_refresh_marker(&mut session, &catalogs);

        assert_eq!(
            session
                .get("configCatalogRefreshRequiredAt")
                .and_then(serde_json::Value::as_str),
            Some("200Z")
        );
    }

    #[test]
    fn session_catalog_wins_ties_and_unavailable_values_are_structured() {
        assert!(!acp_catalog_observation_is_newer("200Z", Some("200Z")));
        assert!(!acp_catalog_observation_is_newer("199Z", Some("200Z")));
        assert!(acp_catalog_observation_is_newer("201Z", Some("200Z")));

        let catalog = AcpSessionConfigCatalog::from_value(
            &serde_json::json!({
                "modes": { "availableModes": [{ "id": "ask" }] }
            }),
            Some("200Z".to_string()),
        );
        let error = validate_acp_catalog_mode(&catalog, "full").unwrap_err();
        assert_eq!(
            error.code,
            gold_band::acp::client::ACP_SESSION_CONFIG_VALUE_UNAVAILABLE_CODE
        );
        assert_eq!(error.params["category"], "mode");
        assert_eq!(error.params["value"], "full");
        assert_eq!(error.params["availableValues"], serde_json::json!(["ask"]));
    }
    use std::sync::{Arc, Mutex};

    fn bound_authoring(agent_id: &str, workflow_id: &str) -> TaskAuthoringWorkflow {
        TaskAuthoringWorkflow {
            workflow: WorkflowDsl {
                version: gold_band::domain::VERSION.to_string(),
                id: workflow_id.to_string(),
                entry: "dev".to_string(),
                control: Default::default(),
                nodes: vec![NodeDsl::Worker(WorkerNode {
                    id: "dev".to_string(),
                    execution_slot_id: Some("slot-dev".to_string()),
                    provider: None,
                    model: None,
                    profile: None,
                    goal: None,
                    output: None,
                    success_condition: None,
                    permission_mode: None,
                    config_options: BTreeMap::new(),
                    manual_check: None,
                    prompt_envelope: Default::default(),
                })],
                edges: Vec::new(),
            },
            model_bindings: WorkflowModelBindings {
                bindings: vec![WorkerModelBinding {
                    execution_slot_id: "slot-dev".to_string(),
                    agent_id: agent_id.to_string(),
                    model_id: None,
                    permission_mode_id: None,
                    config_options: BTreeMap::new(),
                }],
                ..WorkflowModelBindings::default()
            },
        }
    }

    fn dynamic_authoring(agent_id: &str, workflow_id: &str) -> TaskAuthoringWorkflow {
        TaskAuthoringWorkflow {
            workflow: WorkflowDsl {
                version: gold_band::domain::VERSION.to_string(),
                id: workflow_id.to_string(),
                entry: "route".to_string(),
                control: Default::default(),
                nodes: vec![NodeDsl::AiDynamic(gold_band::dsl::AiDynamicNode {
                    id: "route".to_string(),
                    agent_strategy: AiDynamicAgentStrategy::Dynamic {
                        bootstrap_provider: agent_id.to_string(),
                        bootstrap_model: None,
                        permission_mode: None,
                        bootstrap_config_options: Default::default(),
                        acceptance_model: None,
                        acceptance_config_options: Default::default(),
                        routing_prompt: "route by task".to_string(),
                        available_agents: vec![gold_band::dsl::DynamicAgentRef {
                            provider: "agent-b".to_string(),
                            model: None,
                            permission_mode: None,
                            config_options: Default::default(),
                        }],
                    },
                    config_options: Default::default(),
                    allowed_profiles: Vec::new(),
                    global_goal: None,
                    control: gold_band::dsl::DynamicControlDsl::default(),
                    allowed_workflows: Vec::new(),
                })],
                edges: Vec::new(),
            },
            model_bindings: WorkflowModelBindings::default(),
        }
    }

    fn write_bound_task(app: &App, task_id: &str, agent_id: &str) {
        write_json(&app.paths.task_file(task_id), &TaskState::new(task_id)).unwrap();
        write_json(
            &app.paths.workflow_file(task_id),
            &bound_authoring(agent_id, &format!("workflow-{task_id}")),
        )
        .unwrap();
    }

    fn prompt_with_quote(source_message_key: &str, text: &str) -> ConversationPromptInput {
        ConversationPromptInput {
            display_text: "继续".to_string(),
            quotes: vec![gold_band::provider::UserPromptQuote {
                id: "quote-1".to_string(),
                source_message_key: source_message_key.to_string(),
                text: text.to_string(),
            }],
        }
    }

    #[test]
    fn prompt_quote_validation_enforces_bounded_shape_without_loading_sources() {
        let valid = prompt_with_quote("textDelta-answer-1", "Agent 原文");
        assert!(validate_conversation_prompt_input(&valid, None).is_ok());

        let mut duplicate = valid.clone();
        duplicate.quotes.push(duplicate.quotes[0].clone());
        assert_eq!(
            validate_conversation_prompt_input(&duplicate, None)
                .unwrap_err()
                .code,
            "conversation.prompt-quote-invalid"
        );

        let over_limit = prompt_with_quote(
            "textDelta-answer-1",
            &"字".repeat(MAX_USER_PROMPT_QUOTE_CHARS + 1),
        );
        assert_eq!(
            validate_conversation_prompt_input(&over_limit, None)
                .unwrap_err()
                .code,
            "conversation.prompt-quote-limit-exceeded"
        );
        let mut too_many = valid.clone();
        too_many.quotes = (0..=MAX_USER_PROMPT_QUOTES)
            .map(|index| gold_band::provider::UserPromptQuote {
                id: format!("quote-{index}"),
                source_message_key: format!("source-{index}"),
                text: "x".to_string(),
            })
            .collect();
        assert_eq!(
            validate_conversation_prompt_input(&too_many, None)
                .unwrap_err()
                .code,
            "conversation.prompt-quote-count-exceeded"
        );

        let too_long_id = ConversationPromptInput {
            display_text: "继续".to_string(),
            quotes: vec![gold_band::provider::UserPromptQuote {
                id: "x".repeat(MAX_USER_PROMPT_QUOTE_ID_BYTES + 1),
                source_message_key: "arbitrary-source".to_string(),
                text: "用户提供的任意引用内容".to_string(),
            }],
        };
        assert_eq!(
            validate_conversation_prompt_input(&too_long_id, None)
                .unwrap_err()
                .code,
            "conversation.prompt-quote-metadata-too-long"
        );

        assert!(
            validate_conversation_prompt_input(
                &prompt_with_quote("code-selection:anywhere", "引用不需要在消息时间线中存在",),
                None,
            )
            .is_ok()
        );
    }

    #[test]
    fn prompt_payload_validation_accepts_attachment_only_and_rejects_fully_empty_input() {
        let temp = tempfile::tempdir().unwrap();
        let attachment = temp.path().join("context.txt");
        std::fs::write(&attachment, "attachment content").unwrap();
        let attachment = attachment.to_string_lossy().to_string();
        let input = ConversationPromptInput {
            display_text: String::new(),
            quotes: Vec::new(),
        };

        assert_eq!(
            validate_conversation_prompt_input(&input, None)
                .unwrap_err()
                .code,
            "conversation.prompt-empty"
        );
        assert!(
            validate_conversation_prompt_input(&input, Some(std::slice::from_ref(&attachment)))
                .is_ok()
        );
    }

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
    fn blocking_command_preserves_panicking_join_diagnostics() {
        let error = tauri::async_runtime::block_on(async {
            spawn_blocking_command::<(), _>(|| {
                panic!("simulated blocking command panic");
            })
            .await
            .unwrap_err()
        });

        assert_eq!(error.code, "app.task-join-failed");
        assert_eq!(error.params["kind"], "panic");
        assert!(
            error.params["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("simulated blocking command panic"))
        );
    }

    #[test]
    fn runtime_workspace_validation_runs_outside_the_caller_thread() {
        let caller_thread = std::thread::current().id();

        let worker_thread = tauri::async_runtime::block_on(async {
            run_runtime_workspace_validation(|| Ok(std::thread::current().id()))
                .await
                .unwrap()
        });

        assert_ne!(worker_thread, caller_thread);
    }

    #[test]
    fn personalization_font_stack_validation_preserves_order_and_rejects_invalid_input() {
        let mut valid = PersonalizationPreference::default();
        valid.typography.ui.font_stack = FontStackPreference::Custom {
            families: vec![" Segoe UI ".to_string(), "Gold Band MiSans".to_string()],
        };
        let normalized = normalize_personalization_preference(valid).unwrap();
        assert_eq!(
            normalized.typography.ui.font_stack,
            FontStackPreference::Custom {
                families: vec!["Segoe UI".to_string(), "Gold Band MiSans".to_string()],
            }
        );

        for families in [
            vec![],
            vec!["Segoe UI".to_string(), "segoe ui".to_string()],
            vec!["bad,font".to_string()],
            vec!["x".repeat(MAX_FONT_FAMILY_CHARS + 1)],
        ] {
            let mut invalid = PersonalizationPreference::default();
            invalid.typography.ui.font_stack = FontStackPreference::Custom { families };
            assert_eq!(
                normalize_personalization_preference(invalid)
                    .unwrap_err()
                    .code,
                "personalization.font-stack-invalid"
            );
        }
    }

    #[test]
    fn agent_usage_workspace_apps_include_registered_and_current_projects_once() {
        let root = tempfile::tempdir().unwrap();
        let current = Utf8PathBuf::from_path_buf(root.path().join("current")).unwrap();
        let other = Utf8PathBuf::from_path_buf(root.path().join("other")).unwrap();
        let context = crate::state::DesktopContext {
            repo_root: current.clone(),
            config: RuntimeConfig::default(),
            recent_workspaces: Vec::new(),
            needs_workspace: false,
        };
        let workspaces = vec![
            ConversationWorkspaceEntry {
                project_id: "current-alias".to_string(),
                workspace_path: current.to_string(),
                name: "Current".to_string(),
                added_at: String::new(),
            },
            ConversationWorkspaceEntry {
                project_id: "other".to_string(),
                workspace_path: other.to_string(),
                name: "Other".to_string(),
                added_at: String::new(),
            },
            ConversationWorkspaceEntry {
                project_id: "other-duplicate".to_string(),
                workspace_path: other.to_string(),
                name: "Other duplicate".to_string(),
                added_at: String::new(),
            },
        ];

        let apps = agent_usage_workspace_apps(&context, &workspaces).unwrap();
        let project_ids = apps
            .iter()
            .map(|app| app.paths.project_id.clone())
            .collect::<BTreeSet<_>>();

        assert_eq!(apps.len(), 2);
        assert_eq!(project_ids.len(), 2);
        assert!(project_ids.contains(&context.app().paths.project_id));
    }

    #[test]
    fn agent_binding_usage_aggregates_tasks_and_schedules_across_workspaces() {
        let root = tempfile::tempdir().unwrap();
        let first =
            App::new(Utf8PathBuf::from_path_buf(root.path().join("first-workspace")).unwrap());
        let second =
            App::new(Utf8PathBuf::from_path_buf(root.path().join("second-workspace")).unwrap());
        write_bound_task(&first, "task-first", "agent-a");
        write_bound_task(&second, "task-second", "agent-a");
        write_bound_task(&second, "task-unrelated", "agent-b");
        write_json(
            &first.paths.task_file("task-dynamic"),
            &TaskState::new("task-dynamic"),
        )
        .unwrap();
        write_json(
            &first.paths.workflow_file("task-dynamic"),
            &dynamic_authoring("agent-a", "workflow-task-dynamic"),
        )
        .unwrap();

        let database = ScheduledTaskDatabase::open(second.paths.scheduler_db_path()).unwrap();
        let mut scheduled_workflow = ScheduledTaskDefinition::new(
            &second.paths.project_id,
            "scheduled-agent-a",
            "workflow",
            ScheduleSpec::every(1, "hours", chrono::Utc::now()).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        scheduled_workflow.content_snapshot.workflow_authoring =
            Some(serde_json::to_value(bound_authoring("agent-a", "scheduled-workflow")).unwrap());
        database.save_job_definition(&scheduled_workflow).unwrap();

        let mut scheduled_direct = ScheduledTaskDefinition::new(
            &second.paths.project_id,
            "scheduled-direct-agent-a",
            "direct",
            ScheduleSpec::every(1, "hours", chrono::Utc::now()).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        scheduled_direct.content_snapshot.direct_agent_id = Some("agent-a".to_string());
        database.save_job_definition(&scheduled_direct).unwrap();

        let mut scheduled_auto = ScheduledTaskDefinition::new(
            &second.paths.project_id,
            "scheduled-auto-agent-a",
            "auto",
            ScheduleSpec::every(1, "hours", chrono::Utc::now()).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        scheduled_auto.content_snapshot.auto_authoring =
            Some(gold_band::scheduler::AutoAuthoringIdentity::new(
                "agent-a",
                "fixed",
                None::<String>,
                Vec::<String>::new(),
                None::<String>,
                Vec::<String>::new(),
            ));
        database.save_job_definition(&scheduled_auto).unwrap();

        let template_authoring = bound_authoring("agent-a", "template-workflow");
        let dynamic_template_authoring = dynamic_authoring("agent-a", "template-dynamic-workflow");
        let templates = WorkflowTemplateStore {
            version: gold_band::domain::VERSION.to_string(),
            last_used_template_id: None,
            last_created_workflow: None,
            templates: vec![
                WorkflowTemplate {
                    id: "template-a".to_string(),
                    name: "Template A".to_string(),
                    is_built_in: false,
                    optional_entry_stage: None,
                    workflow: template_authoring.workflow,
                    model_bindings: template_authoring.model_bindings,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                WorkflowTemplate {
                    id: "template-dynamic".to_string(),
                    name: "Dynamic Template".to_string(),
                    is_built_in: false,
                    optional_entry_stage: None,
                    workflow: dynamic_template_authoring.workflow,
                    model_bindings: dynamic_template_authoring.model_bindings,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            ],
        };
        let agent_id = ManagedAgentId::from_str("agent-a").unwrap();

        let usage = collect_agent_binding_usage(&agent_id, &templates, &[first, second]).unwrap();

        assert_eq!(
            usage,
            AgentBindingUsageVm {
                workflow_template_count: 2,
                task_count: 3,
                scheduled_task_count: 3,
                unknown_task_count: 0,
                unknown_scheduled_task_count: 0,
            }
        );
    }

    #[test]
    fn scheduled_attention_lookup_only_requires_coordinator_for_matching_occurrence() {
        let directory = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = App::new(root);

        assert!(
            !scheduled_attention_requires_coordinator(
                &app,
                "task-1",
                "run-1",
                "round-1",
                "attempt-1",
            )
            .unwrap()
        );
        assert!(!app.paths.scheduler_db_path().exists());

        let database =
            gold_band::scheduler::db::ScheduledTaskDatabase::open(app.paths.scheduler_db_path())
                .unwrap();
        let now = chrono::Utc::now();
        let definition = gold_band::scheduler::ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "job-1",
            "direct",
            gold_band::scheduler::ScheduleSpec::at(now + chrono::Duration::hours(1)),
            gold_band::scheduler::OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        database
            .create_job(&definition, Some(now + chrono::Duration::hours(1)))
            .unwrap();
        let occurrence = database
            .create_or_get_occurrence_for_existing_job(
                &app.paths.project_id,
                definition.id(),
                now,
                gold_band::scheduler::occurrence::OccurrenceTriggerKind::Manual,
            )
            .unwrap()
            .unwrap();
        let owner_id = "owner-1";
        database
            .claim_occurrence(
                &app.paths.project_id,
                &occurrence.id,
                owner_id,
                now,
                now + chrono::Duration::minutes(5),
            )
            .unwrap();
        database
            .finish_occurrence(
                &app.paths.project_id,
                &occurrence.id,
                owner_id,
                gold_band::scheduler::occurrence::OccurrenceStatus::AttentionRequired,
                Some(gold_band::scheduler::occurrence::OccurrenceLinks {
                    task_id: Some("task-1".to_string()),
                    run_id: Some("run-1".to_string()),
                    round_id: Some("round-1".to_string()),
                    attempt_id: Some("attempt-1".to_string()),
                }),
                Some(gold_band::scheduler::occurrence::ScheduledError::new(
                    gold_band::scheduler::occurrence::ScheduledErrorCode::UserInputRequired,
                )),
            )
            .unwrap();

        assert!(
            scheduled_attention_requires_coordinator(
                &app,
                "task-1",
                "run-1",
                "round-1",
                "attempt-1",
            )
            .unwrap()
        );
    }

    #[test]
    fn scheduled_attention_lookup_maps_database_failure_to_storage_error() {
        let directory = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
        let app = App::new(root);
        std::fs::create_dir_all(app.paths.scheduler_db_path()).unwrap();

        let error = scheduled_attention_requires_coordinator(
            &app,
            "task-1",
            "run-1",
            "round-1",
            "attempt-1",
        )
        .unwrap_err();

        assert_eq!(
            error.code,
            gold_band::scheduler::occurrence::ScheduledErrorCode::StorageFailed
        );
    }

    #[test]
    fn agent_binding_usage_isolates_damaged_tasks_and_scheduled_snapshots() {
        let root = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(root.path().join("workspace")).unwrap());
        write_bound_task(&app, "task-valid", "agent-a");
        write_json(
            &app.paths.task_file("task-invalid"),
            &TaskState::new("task-invalid"),
        )
        .unwrap();
        std::fs::create_dir_all(
            app.paths
                .workflow_file("task-invalid")
                .parent()
                .unwrap()
                .as_std_path(),
        )
        .unwrap();
        std::fs::write(
            app.paths.workflow_file("task-invalid").as_std_path(),
            "{invalid",
        )
        .unwrap();

        let database = ScheduledTaskDatabase::open(app.paths.scheduler_db_path()).unwrap();
        let mut valid = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "scheduled-valid",
            "workflow",
            ScheduleSpec::every(1, "hours", chrono::Utc::now()).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        valid.content_snapshot.workflow_authoring =
            Some(serde_json::to_value(bound_authoring("agent-a", "scheduled-valid")).unwrap());
        database.save_job_definition(&valid).unwrap();

        let mut invalid_snapshot = ScheduledTaskDefinition::new(
            &app.paths.project_id,
            "scheduled-invalid-snapshot",
            "workflow",
            ScheduleSpec::every(1, "hours", chrono::Utc::now()).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        invalid_snapshot.content_snapshot.workflow_authoring = Some(serde_json::json!({
            "workflow": "invalid"
        }));
        database.save_job_definition(&invalid_snapshot).unwrap();

        drop(database);

        let usage = collect_agent_binding_usage(
            &ManagedAgentId::from_str("agent-a").unwrap(),
            &WorkflowTemplateStore {
                version: gold_band::domain::VERSION.to_string(),
                last_used_template_id: None,
                last_created_workflow: None,
                templates: Vec::new(),
            },
            &[app],
        )
        .unwrap();

        assert_eq!(usage.workflow_template_count, 0);
        assert_eq!(usage.task_count, 1);
        assert_eq!(usage.scheduled_task_count, 1);
        assert_eq!(usage.unknown_task_count, 1);
        assert_eq!(usage.unknown_scheduled_task_count, 1);
    }

    #[test]
    fn workflow_agent_usage_covers_legacy_worker_and_ai_dynamic_roles() {
        let mut legacy_worker = bound_authoring("agent-b", "legacy-worker");
        legacy_worker.model_bindings.bindings.clear();
        let NodeDsl::Worker(worker) = &mut legacy_worker.workflow.nodes[0] else {
            panic!("expected worker node");
        };
        worker.provider = Some("agent-a".to_string());

        let fixed_dynamic = TaskAuthoringWorkflow {
            workflow: WorkflowDsl {
                version: gold_band::domain::VERSION.to_string(),
                id: "fixed-dynamic".to_string(),
                entry: "route".to_string(),
                control: Default::default(),
                nodes: vec![NodeDsl::AiDynamic(gold_band::dsl::AiDynamicNode {
                    id: "route".to_string(),
                    agent_strategy: AiDynamicAgentStrategy::Fixed {
                        provider: "agent-a".to_string(),
                        model: None,
                        permission_mode: None,
                    },
                    config_options: Default::default(),
                    allowed_profiles: Vec::new(),
                    global_goal: None,
                    control: gold_band::dsl::DynamicControlDsl::default(),
                    allowed_workflows: Vec::new(),
                })],
                edges: Vec::new(),
            },
            model_bindings: WorkflowModelBindings::default(),
        };
        let dynamic_available = dynamic_authoring("agent-c", "dynamic-available");

        assert!(workflow_references_agent(
            &legacy_worker.workflow,
            &legacy_worker.model_bindings,
            "agent-a"
        ));
        assert!(workflow_references_agent(
            &fixed_dynamic.workflow,
            &fixed_dynamic.model_bindings,
            "agent-a"
        ));
        assert!(workflow_references_agent(
            &dynamic_available.workflow,
            &dynamic_available.model_bindings,
            "agent-b"
        ));
    }

    #[test]
    fn auto_authoring_usage_covers_current_secondary_and_legacy_identity_fields() {
        let current = gold_band::scheduler::AutoAuthoringIdentity::new(
            "agent-a",
            "fixed",
            None::<String>,
            Vec::<String>::new(),
            None::<String>,
            Vec::<String>::new(),
        );
        let secondary = gold_band::scheduler::AutoAuthoringIdentity::new(
            "agent-b",
            "dynamic",
            Some("agent-a"),
            vec!["agent-a"],
            None::<String>,
            Vec::<String>::new(),
        );
        let legacy = gold_band::scheduler::AutoAuthoringIdentity {
            agent_strategy: "agent-a".to_string(),
            agent_type: "fixed".to_string(),
            bootstrap_agent_type: None,
            available_agent_types: Vec::new(),
            global_goal: None,
            allowed_workflow_ids: Vec::new(),
        };

        assert!(auto_authoring_references_agent(&current, "agent-a"));
        assert!(auto_authoring_references_agent(&secondary, "agent-a"));
        assert!(auto_authoring_references_agent(&legacy, "agent-a"));
        assert!(!auto_authoring_references_agent(&legacy, "agent-b"));
    }

    #[test]
    fn personalization_wallpaper_validation_rejects_invalid_identity_and_opacity() {
        let mut invalid_identity = PersonalizationPreference::default();
        invalid_identity
            .wallpaper
            .for_color_scheme_mut(ResolvedColorScheme::Light)
            .image = WallpaperImagePreference::User {
            asset_id: "  ".to_string(),
        };
        assert_eq!(
            normalize_personalization_preference(invalid_identity)
                .unwrap_err()
                .code,
            "personalization.wallpaper-invalid"
        );

        for opacity_percent in [0, 19, 101] {
            let mut invalid_opacity = PersonalizationPreference::default();
            invalid_opacity
                .wallpaper
                .for_color_scheme_mut(ResolvedColorScheme::Dark)
                .opacity_percent = opacity_percent;
            assert_eq!(
                normalize_personalization_preference(invalid_opacity)
                    .unwrap_err()
                    .code,
                "personalization.wallpaper-opacity-invalid"
            );
        }
    }

    #[test]
    fn live_event_envelope_serializes_generation_without_revision() {
        let (timeline_generation, timeline_revision) =
            acp_timeline_position_fields(Some(AcpLiveTimelinePosition::transient(7)));
        let envelope = AcpSessionUpdatedEventVm {
            branch_id: Some("root".to_string()),
            timeline_generation,
            timeline_revision,
            project_id: Some("project-a".to_string()),
            task_id: "task-a".to_string(),
            task_uuid: Some("task-uuid-a".to_string()),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "direct-agent".to_string(),
            attempt_id: "attempt-001".to_string(),
            outer_node_id: None,
            outer_attempt_id: None,
            session: None,
            event: Some(AcpUiEvent {
                id: "acp-timing-1".to_string(),
                seq: 1,
                timestamp: "2026-08-31T00:00:00Z".to_string(),
                kind: "timingUpdate".to_string(),
                session_id: Some("session-1".to_string()),
                content: None,
                title: None,
                tool_call_id: None,
                status: Some("active".to_string()),
                started_seq: None,
                ended_seq: None,
                started_at: None,
                ended_at: None,
                timing: None,
                raw: None,
            }),
            lifecycle: None,
            activity: None,
            task_activity_at: None,
        };

        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(value["timelineGeneration"], 7);
        assert!(value["timelineRevision"].is_null());
    }

    #[test]
    fn acp_session_update_serializes_lightweight_prompt_activity_and_terminal_clear() {
        let active = AcpSessionUpdatedEventVm {
            branch_id: None,
            timeline_generation: None,
            timeline_revision: None,
            project_id: Some("project-a".to_string()),
            task_id: "task-a".to_string(),
            task_uuid: Some("task-uuid-a".to_string()),
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
            task_activity_at: Some("2026-08-29T12:00:00Z".to_string()),
        };
        let active_json = serde_json::to_value(active).unwrap();
        assert_eq!(
            active_json["activity"],
            serde_json::json!({ "phase": "running", "stopping": false })
        );
        assert!(active_json["lifecycle"].is_null());
        assert_eq!(active_json["taskUuid"], "task-uuid-a");
        assert_eq!(active_json["taskActivityAt"], "2026-08-29T12:00:00Z");

        let terminal = AcpSessionUpdatedEventVm {
            branch_id: None,
            timeline_generation: None,
            timeline_revision: None,
            project_id: Some("project-a".to_string()),
            task_id: "task-a".to_string(),
            task_uuid: Some("task-uuid-a".to_string()),
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
            task_activity_at: None,
        };
        assert!(serde_json::to_value(terminal).unwrap()["activity"].is_null());
    }

    #[test]
    fn repeated_finished_observation_reuses_the_canonical_terminal_activity_time() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-task-activity-terminal-idempotency-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "direct-agent".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let terminal_at = "2026-08-30T10:00:00Z";
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
                "includeOptionalEntry": false,
                "directConfig": null,
                "agentIdentity": null,
                "titleAutoGenerated": false,
                "initialAttachmentNames": null,
                "createdAt": "2026-08-30T09:00:00Z",
                "lastActivityAt": terminal_at
            }),
        )
        .unwrap();
        write_json(
            &acp_lifecycle_path(&locator.attempt_dir(&app)),
            &serde_json::json!({
                "availability": "established",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "cancelled",
                "acpRevision": 2,
                "turnId": "turn-001",
                "lifecycleOperationId": "stop-001",
                "stopReason": "cancelled",
                "restored": false,
                "capabilities": {},
                "createdAt": "2026-08-30T09:00:00Z",
                "updatedAt": terminal_at
            }),
        )
        .unwrap();

        process_prompt_turn_lifecycle(
            &app,
            locator,
            AcpPromptLifecycleEvent::Finished {
                prompt_id: Some("turn-001".to_string()),
                successful: false,
            },
            |_, _, _| {},
        )
        .unwrap();

        let metadata: serde_json::Value = read_json(
            &app.paths
                .task_dir("task-001")
                .join("authoring")
                .join("conversation.json"),
        )
        .unwrap();
        assert_eq!(metadata["lastActivityAt"], terminal_at);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn task_activity_index_update_preserves_search_document_and_repairs_missing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        let index = gold_band::storage::sqlite::SearchIndex::open(&db_path).unwrap();
        let tasks_dir = Utf8PathBuf::from_path_buf(dir.path().join("tasks")).unwrap();
        let schema = rusqlite::Connection::open(db_path.as_std_path()).unwrap();
        let task_update_trigger_sql: String = schema
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'tasks_au'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            task_update_trigger_sql
                .to_ascii_lowercase()
                .contains("after update of title, description, requirement_text on tasks")
        );

        let indexed_task = tasks_dir.join("task-001");
        std::fs::create_dir_all(indexed_task.join("authoring").as_std_path()).unwrap();
        write_json(
            &indexed_task.join("task.json"),
            &TaskState {
                version: gold_band::domain::VERSION.to_string(),
                id: "task-001".to_string(),
                title: Some("Indexed title".to_string()),
                description: Some("Indexed description".to_string()),
                uuid: None,
            },
        )
        .unwrap();
        std::fs::write(
            indexed_task
                .join("authoring")
                .join("requirement.md")
                .as_std_path(),
            "indexed-requirement-needle",
        )
        .unwrap();
        index.index_task_with_retry(&indexed_task, "task-001");
        std::fs::remove_file(indexed_task.join("task.json").as_std_path()).unwrap();
        std::fs::remove_file(
            indexed_task
                .join("authoring")
                .join("requirement.md")
                .as_std_path(),
        )
        .unwrap();
        index.index_task_activity_with_retry(&indexed_task, "task-001", "2026-08-30T12:00:00Z");
        let preserved = index
            .search_tasks_in_task_roots("indexed-requirement-needle", &[tasks_dir.to_string()], 10)
            .unwrap();
        assert_eq!(preserved.len(), 1);
        assert_eq!(preserved[0].task_id, "task-001");

        let missing_row_task = tasks_dir.join("task-002");
        std::fs::create_dir_all(missing_row_task.join("authoring").as_std_path()).unwrap();
        write_json(
            &missing_row_task.join("task.json"),
            &TaskState {
                version: gold_band::domain::VERSION.to_string(),
                id: "task-002".to_string(),
                title: Some("Missing row repair".to_string()),
                description: None,
                uuid: None,
            },
        )
        .unwrap();
        index.index_task_activity_with_retry(&missing_row_task, "task-002", "2026-08-30T13:00:00Z");
        let repaired = index
            .search_tasks_in_task_roots("missing row repair", &[tasks_dir.to_string()], 10)
            .unwrap();
        assert_eq!(repaired.len(), 1);
        assert_eq!(repaired[0].task_id, "task-002");
    }

    #[test]
    fn task_activity_schema_v5_upgrade_only_replaces_the_task_update_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = Utf8PathBuf::from_path_buf(dir.path().join("search.db")).unwrap();
        drop(gold_band::storage::sqlite::SearchIndex::open(&db_path).unwrap());
        let connection = rusqlite::Connection::open(db_path.as_std_path()).unwrap();
        connection
            .execute_batch(
                "PRAGMA user_version = 5;
                 DROP TRIGGER tasks_au;
                 CREATE TRIGGER tasks_au AFTER UPDATE ON tasks BEGIN
                    INSERT INTO tasks_fts(tasks_fts, rowid, title, description, requirement_text)
                    VALUES('delete', old.rowid, old.title, old.description, old.requirement_text);
                    INSERT INTO tasks_fts(rowid, title, description, requirement_text)
                    VALUES (new.rowid, new.title, new.description, new.requirement_text);
                 END;
                 INSERT INTO sessions (attempt_path, task_id, run_id, round_id, node_id, attempt_id)
                 VALUES ('attempt-before-v6', 'task-001', 'run-001', 'round-001', 'node-001', 'attempt-001');",
            )
            .unwrap();
        drop(connection);

        drop(gold_band::storage::sqlite::SearchIndex::open(&db_path).unwrap());
        let migrated = rusqlite::Connection::open(db_path.as_std_path()).unwrap();
        let schema_version: i32 = migrated
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let session_count: i64 = migrated
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        let task_update_trigger_sql: String = migrated
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'tasks_au'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(schema_version, 6);
        assert_eq!(session_count, 1);
        assert!(
            task_update_trigger_sql
                .to_ascii_lowercase()
                .contains("after update of title, description, requirement_text on tasks")
        );
    }

    #[test]
    fn startup_stop_pauses_current_runtime_without_synthesizing_an_acp_turn() {
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
        let (attempt_dir, stop_owner, accepted) =
            persist_active_session_stop(&app, &locator, "operation-001").unwrap();

        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(accepted);
        assert!(stop_owner.is_none());
        let run: serde_json::Value = read_json(&app.paths.run_file("task-001", "run-001")).unwrap();
        assert_eq!(run["status"], "paused");
        assert_eq!(run["pause_reason"], "process-interrupted");
        let snapshot: serde_json::Value =
            read_json(&attempt_dir.join("acp.snapshot.json")).unwrap();
        assert!(snapshot.get("turnId").is_none());
        assert!(snapshot.get("promptSubmission").is_none());
        assert_eq!(snapshot["liveTurnActivity"], "idle");
        assert_eq!(snapshot["latestTurnStatus"], "none");
        assert!(timeline_path.is_dir());
    }

    #[test]
    fn dynamic_resume_starting_owner_prevents_stop_from_being_treated_as_noop() {
        assert!(active_session_stop_is_idempotent_noop(false, false, false));
        assert!(!active_session_stop_is_idempotent_noop(false, false, true));
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
    fn existing_attempt_prompt_ignores_worker_creation_mode_and_continues_provider_session() {
        let continue_ref = serde_json::json!({ "acpSessionId": "session-1" });
        let worker_ref = WorkerRefState {
            version: gold_band::domain::VERSION.to_string(),
            provider: "codex".to_string(),
            mode: SessionMode::New,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref: Some(continue_ref.clone()),
            open_command: None,
        };

        let (session_mode, resolved_continue_ref) =
            existing_attempt_prompt_session_target(Some(worker_ref));

        assert_eq!(session_mode, SessionMode::Continue);
        assert_eq!(resolved_continue_ref, Some(continue_ref));

        let (missing_ref_mode, missing_ref) = existing_attempt_prompt_session_target(None);
        assert_eq!(missing_ref_mode, SessionMode::Continue);
        assert_eq!(missing_ref, None);
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
    fn current_acp_session_model_override_prefers_explicit_override() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-model-override-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "adapterId": "t",
                "adapterDisplayName": "T",
                "cwd": ".",
                "status": "ok",
                "restored": false,
                "capabilities": {},
                "createdAt": "",
                "updatedAt": "",
                "models": { "currentModelId": "agent-default" },
                "configOptions": [
                    { "id": "model", "currentValue": "agent-default" }
                ]
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_model_override(&attempt_dir).as_deref(),
            Some("agent-default")
        );

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "adapterId": "t",
                "adapterDisplayName": "T",
                "cwd": ".",
                "status": "ok",
                "restored": false,
                "capabilities": {},
                "createdAt": "",
                "updatedAt": "",
                "modelOverride": "override-default",
                "models": { "currentModelId": "agent-default" },
                "configOptions": [
                    { "id": "model", "currentValue": "agent-default" }
                ]
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_model_override(&attempt_dir).as_deref(),
            Some("override-default")
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn current_acp_session_model_name_resolves_display_name() {
        let dir = std::env::temp_dir().join(format!(
            "gold-band-model-name-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();

        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({
                "adapterId": "t",
                "adapterDisplayName": "T",
                "cwd": ".",
                "status": "ok",
                "restored": false,
                "capabilities": {},
                "createdAt": "",
                "updatedAt": "",
                "modelOverride": "opus",
                "configOptions": [
                    {
                        "id": "model",
                        "currentValue": "opus",
                        "options": [
                            { "value": "opus", "name": "glm-5.2" }
                        ]
                    }
                ]
            }),
        )
        .unwrap();
        assert_eq!(
            current_acp_session_model_override(&attempt_dir).as_deref(),
            Some("opus")
        );
        assert_eq!(
            current_acp_session_model_name(&attempt_dir).as_deref(),
            Some("glm-5.2")
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
    fn direct_queue_drain_keeps_the_originating_scheduled_turn_context() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-direct-drain-context-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap())
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()));

        let drain_app = direct_prompt_queue_drain_app(&app);

        assert_eq!(drain_app.scheduled_occurrence_id(), Some("occurrence-001"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_user_turn_clears_only_the_scheduled_origin() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-queued-turn-context-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let seen = Arc::new(Mutex::new(0usize));
        let seen_for_callback = seen.clone();
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap())
            .with_scheduled_occurrence_id(Some("occurrence-001".to_string()))
            .with_prompt_turn_lifecycle(Arc::new(move |_, _| {
                *seen_for_callback.lock().unwrap() += 1;
                Ok(())
            }));

        let queued_app = queued_user_turn_app(&app);
        queued_app
            .notify_prompt_turn_finished(
                acp_live_event_context(
                    "task-001",
                    Some("task-uuid-001".to_string()),
                    "run-001",
                    "round-001",
                    "node-001",
                    "attempt-001",
                    None,
                    None,
                ),
                Some("turn-001".to_string()),
                true,
            )
            .unwrap();

        assert_eq!(queued_app.scheduled_occurrence_id(), None);
        assert_eq!(*seen.lock().unwrap(), 1);
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
            Some("task-uuid-001".to_string()),
            "run-001",
            "round-001",
            "dev",
            "attempt-001",
            None,
            None,
        );

        assert_eq!(context.task_id, "task-001");
        assert_eq!(context.task_uuid.as_deref(), Some("task-uuid-001"));
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
            Some("task-uuid-001".to_string()),
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
    fn prompt_lifecycle_acceptance_and_completion_drain_every_queued_turn() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-prompt-lifecycle-drain-test-{}",
            uuid::Uuid::new_v4()
        ));
        let repo_root = Utf8PathBuf::from_path_buf(root.clone()).unwrap();
        let app = App::new(repo_root);
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "direct-agent".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let attempt_dir = locator.attempt_dir(&app);
        let expected_contents = ["first", "second", "third"];
        for content in expected_contents {
            enqueue_prompt(&attempt_dir, content.to_string(), Vec::new()).unwrap();
        }

        let queue = load_prompt_queue(&attempt_dir).unwrap();
        let mut current = match claim_next_for_auto_dispatch(&attempt_dir, queue.revision).unwrap()
        {
            AutoClaimResult::Claimed(item) => Some(item),
            result => panic!("expected first queued prompt, got {result:?}"),
        };
        let mut dispatched_contents = Vec::new();

        while let Some(item) = current.take() {
            dispatched_contents.push(item.content.clone());
            process_prompt_turn_lifecycle(
                &app,
                locator.clone(),
                AcpPromptLifecycleEvent::Accepted {
                    prompt_id: item.prompt_id.clone(),
                },
                |_, _, _| panic!("accepted must not schedule the next prompt"),
            )
            .unwrap();
            assert!(
                load_prompt_queue(&attempt_dir)
                    .unwrap()
                    .items
                    .iter()
                    .all(|queued| queued.prompt_id != item.prompt_id),
                "accepted prompt must leave the queue before its provider turn finishes"
            );

            process_prompt_turn_lifecycle(
                &app,
                locator.clone(),
                AcpPromptLifecycleEvent::Finished {
                    prompt_id: Some(item.prompt_id),
                    successful: true,
                },
                |_, successful, completion| {
                    assert!(successful);
                    assert!(completion.is_some());
                    let queue = load_prompt_queue(&attempt_dir).unwrap();
                    current =
                        match claim_next_for_auto_dispatch(&attempt_dir, queue.revision).unwrap() {
                            AutoClaimResult::Claimed(item) => Some(item),
                            AutoClaimResult::Empty => None,
                            result => panic!("unexpected automatic claim result: {result:?}"),
                        };
                },
            )
            .unwrap();
        }

        assert_eq!(dispatched_contents, expected_contents);
        assert!(load_prompt_queue(&attempt_dir).unwrap().items.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_prompt_acceptance_propagates_queue_storage_failure() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-prompt-lifecycle-acceptance-error-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        let locator = AttemptLocator::new(
            "task-001".to_string(),
            "run-001".to_string(),
            "round-001".to_string(),
            "direct-agent".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let attempt_dir = locator.attempt_dir(&app);
        let queued = enqueue_prompt(&attempt_dir, "queued".to_string(), Vec::new()).unwrap();
        claim_queued_prompt(&attempt_dir, &queued.id).unwrap();
        let queue = load_prompt_queue(&attempt_dir).unwrap();
        let queue_path = attempt_dir.join(gold_band::acp::prompt_queue::PROMPT_QUEUE_FILE_NAME);
        std::fs::write(queue_path.as_std_path(), b"{").unwrap();

        let error = process_prompt_turn_lifecycle(
            &app,
            locator,
            AcpPromptLifecycleEvent::Accepted {
                prompt_id: queued.prompt_id.clone(),
            },
            |_, _, _| panic!("failed acceptance must not schedule the next prompt"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("EOF"));
        write_json(&queue_path, &queue).unwrap();
        complete_accepted_prompt(&attempt_dir, &queued.prompt_id).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_run_state_update_maps_node_and_run_boundaries() {
        let started = conversation_run_state_update_for_event(RuntimeLifecycleEvent::NodeStarted {
            project_id: "project-1".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            run_id: "run-001".to_string(),
            run_uuid: None,
            round_id: "round-001".to_string(),
            round_uuid: None,
            round_index: Some(1),
            node_id: "dev".to_string(),
            node_uuid: None,
            attempt_id: "attempt-001".to_string(),
            repo_root: "D:/workspace".to_string(),
            seq: Some(1),
            node_name: Some("dev".to_string()),
            agent_type: Some("codex-acp".to_string()),
            resolved_model: None,
            started_at: "2026-06-25T00:00:00Z".to_string(),
            attempt_dir: None,
            predecessor: None,
            metrics_unit_kind: None,
            child_run_id: None,
        })
        .unwrap();
        assert_eq!(started.event_kind, "node-started");
        assert_eq!(started.project_id, "project-1");
        assert_eq!(started.node_id, "dev");
        assert_eq!(started.status, RunStatus::Running);
        assert_eq!(started.outcome, None);

        assert!(
            conversation_run_state_update_for_event(RuntimeLifecycleEvent::NodeStarted {
                project_id: "project-1".to_string(),
                task_id: "task-001".to_string(),
                task_uuid: None,
                run_id: "run-001".to_string(),
                run_uuid: None,
                round_id: "round-001".to_string(),
                round_uuid: None,
                round_index: Some(1),
                node_id: "dynamic-worker".to_string(),
                node_uuid: None,
                attempt_id: "attempt-001".to_string(),
                repo_root: "D:/workspace".to_string(),
                seq: None,
                node_name: Some("dynamic-worker".to_string()),
                agent_type: Some("codex-acp".to_string()),
                resolved_model: None,
                started_at: "2026-06-25T00:00:00Z".to_string(),
                attempt_dir: None,
                predecessor: None,
                metrics_unit_kind: Some(gold_band::dynamic::DynamicNodeKind::Worker),
                child_run_id: None,
            })
            .is_none()
        );

        let paused = conversation_run_state_update_for_event(RuntimeLifecycleEvent::RunPaused {
            event_id: "event-paused".to_string(),
            occurred_at: "2026-06-25T00:00:00Z".to_string(),
            scheduled_occurrence_id: None,
            project_id: "project-1".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: Some("task-uuid-001".to_string()),
            run_id: "run-001".to_string(),
            round_id: "round-001".to_string(),
            node_id: "plan".to_string(),
            attempt_id: "attempt-001".to_string(),
            node_label: "plan".to_string(),
            pause_reason: PauseReason::WaitingForUserInput,
            task_title: None,
        })
        .unwrap();
        assert_eq!(paused.event_kind, "run-paused");
        assert_eq!(paused.project_id, "project-1");
        assert_eq!(paused.task_id, "task-001");
        assert_eq!(paused.task_uuid.as_deref(), Some("task-uuid-001"));
        assert_eq!(paused.run_id, "run-001");
        assert_eq!(paused.round_id, "round-001");
        assert_eq!(paused.node_id, "plan");
        assert_eq!(paused.attempt_id, "attempt-001");
        assert_eq!(paused.status, RunStatus::Paused);
        assert_eq!(paused.outcome, None);
        assert_eq!(
            serde_json::to_value(&paused).unwrap()["taskUuid"],
            "task-uuid-001"
        );

        let completed =
            conversation_run_state_update_for_event(RuntimeLifecycleEvent::RunCompleted {
                event_id: "event-completed".to_string(),
                occurred_at: "2026-06-25T00:00:01Z".to_string(),
                scheduled_occurrence_id: None,
                project_id: "project-1".to_string(),
                task_id: "task-001".to_string(),
                task_uuid: Some("task-uuid-001".to_string()),
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
        assert_eq!(completed.event_kind, "run-completed");
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
            worktree: None,
            execution: Default::default(),
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
                "includeOptionalEntry": true,
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
                acp_storage_schema_version: gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
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
        write_json(
            &app.paths
                .round_file(&locator.task_id, &locator.run_id, &locator.round_id),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "id": locator.round_id,
                "run_id": locator.run_id,
                "index": 1,
                "status": "paused",
                "outcome": null,
                "trigger": "initial",
                "started_at": "2026-08-12T00:00:00Z",
                "trace": []
            }),
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
            worktree: None,
            execution: Default::default(),
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
    fn superseded_workflow_attempt_rejects_prompt_with_latest_target_locator() {
        let root = std::env::temp_dir().join(format!(
            "gold-band-superseded-session-test-{}",
            uuid::Uuid::new_v4()
        ));
        let app = App::new(Utf8PathBuf::from_path_buf(root.clone()).unwrap());
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        write_json(
            &app.paths
                .task_dir(task_id)
                .join("authoring")
                .join("conversation.json"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "source": "conversation",
                "runMode": "workflow",
                "workflowTemplateId": "default",
                "includeInterview": false,
                "directConfig": null,
                "agentIdentity": null,
                "titleAutoGenerated": false,
                "initialAttachmentNames": null,
                "createdAt": "2026-08-16T00:00:00Z",
                "lastActivityAt": null
            }),
        )
        .unwrap();
        write_json(
            &app.paths.workflow_snapshot_file(task_id, run_id),
            &serde_json::json!({
                "version": "0.1",
                "id": "session-owner",
                "entry": "review",
                "control": {},
                "nodes": [
                    { "type": "worker", "id": "review", "provider": "claude-acp" }
                ],
                "edges": [
                    { "from": "review", "to": "review", "on": "failure", "session": "continue" }
                ]
            }),
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "completed",
                "outcome": "success",
                "trigger": "initial",
                "started_at": "2026-08-16T00:00:00Z",
                "trace": [
                    { "sequence": 1, "node_id": "review", "attempt_id": "attempt-001", "from_node_id": null, "edge_outcome": null, "entered_at": "2026-08-16T00:00:00Z" },
                    { "sequence": 2, "node_id": "review", "attempt_id": "attempt-002", "from_node_id": "review", "edge_outcome": "failure", "entered_at": "2026-08-16T00:00:01Z" }
                ]
            }),
        )
        .unwrap();
        write_json(
            &app.paths
                .worker_ref_file(task_id, run_id, round_id, "review", "attempt-001"),
            &serde_json::json!({
                "version": gold_band::domain::VERSION,
                "provider": "claude-acp",
                "mode": "new",
                "supports_open_session": true,
                "supports_continue_session": true,
                "continue_ref": { "acpSessionId": "session-001" },
                "open_command": null
            }),
        )
        .unwrap();

        let locator = AttemptLocator::new(
            task_id.to_string(),
            run_id.to_string(),
            round_id.to_string(),
            "review".to_string(),
            "attempt-001".to_string(),
            None,
            None,
        );
        let error = ensure_conversation_prompt_available(&app, &locator).unwrap_err();

        assert_eq!(error.code, "conversation.session-superseded");
        assert_eq!(error.params["roundId"], round_id);
        assert_eq!(error.params["nodeId"], "review");
        assert_eq!(error.params["attemptId"], "attempt-002");
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
                "includeOptionalEntry": null,
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
            worktree: None,
            execution: Default::default(),
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
            worktree: None,
            execution: Default::default(),
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
                "version": gold_band::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION,
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
                "version": gold_band::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION,
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
            worktree: None,
            execution: Default::default(),
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
                "version": gold_band::dynamic_store::CURRENT_DYNAMIC_GRAPH_VERSION,
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
            "turn-1",
            "prompt-event-1",
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
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let attempt_dir = Utf8PathBuf::from_path_buf(dir.clone()).unwrap();
        gold_band::storage::write_json(
            &attempt_dir.join("node.json"),
            &NodeState {
                version: gold_band::domain::VERSION.to_string(),
                acp_storage_schema_version: gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: "direct-agent".to_string(),
                node_type: gold_band::domain::NodeType::Worker,
                run_id: "run-001".to_string(),
                round_id: "round-001".to_string(),
                attempt_id: "attempt-001".to_string(),
                status: RunStatus::Paused,
                outcome: None,
                started_at: "1778771540Z".to_string(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: gold_band::domain::ResolvedConfig::new(),
                uuid: None,
            },
        )
        .unwrap();
        let params = serde_json::json!({ "sessionId": "session-1" });
        gold_band::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            "turn-1",
            "prompt-event-1",
            params.clone(),
            "1778771541Z".to_string(),
        )
        .unwrap();
        let mut pending =
            gold_band::acp::events::permission_request_event(1, "0".to_string(), params);
        pending.id = "permission-0".to_string();
        pending.started_seq = Some(1);
        pending.ended_seq = Some(1);
        gold_band::acp::events::write_timeline_items(
            &attempt_dir.join("acp.timeline.jsonl"),
            &[pending],
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
                task_uuid: None,
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
                task_uuid: None,
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
            task_uuid: None,
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
                task_uuid: None,
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

    #[test]
    fn workflow_intervention_metrics_carry_current_node_context() {
        let temp = std::env::temp_dir().join(format!(
            "gold-band-workflow-intervention-metrics-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        let repo_root = Utf8PathBuf::from_path_buf(temp.join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = App::new(repo_root).with_metrics_collection_enabled(true);
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let started_at = "2026-08-11T00:00:00Z".to_string();
        let task_uuid = uuid::Uuid::new_v4().to_string();
        let run_uuid = uuid::Uuid::new_v4().to_string();
        let node_uuid = uuid::Uuid::new_v4().to_string();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &RunState {
                version: gold_band::domain::VERSION.to_string(),
                id: run_id.to_string(),
                task_id: task_id.to_string(),
                task_uuid: Some(task_uuid),
                status: gold_band::domain::RunStatus::Paused,
                outcome: None,
                started_at: started_at.clone(),
                updated_at: started_at.clone(),
                workflow_snapshot: "workflow.snapshot.json".to_string(),
                current_round: Some(round_id.to_string()),
                current_node: Some("plan".to_string()),
                current_attempt: Some("attempt-001".to_string()),
                new_rounds_opened: 0,
                pause_reason: Some(gold_band::domain::PauseReason::WaitingForUserInput),
                uuid: Some(run_uuid.clone()),
                last_executed_node: None,
                worktree: None,
                execution: Default::default(),
            },
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &RoundState {
                version: gold_band::domain::VERSION.to_string(),
                id: round_id.to_string(),
                run_id: run_id.to_string(),
                index: 1,
                status: gold_band::domain::RunStatus::Running,
                outcome: None,
                trigger: gold_band::domain::RoundTrigger::Initial,
                started_at: started_at.clone(),
                trace: Vec::new(),
                uuid: None,
            },
        )
        .unwrap();
        let mut node = NodeState {
            version: gold_band::domain::VERSION.to_string(),
            acp_storage_schema_version: gold_band::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
            node_id: "plan".to_string(),
            node_type: gold_band::domain::NodeType::Worker,
            run_id: run_id.to_string(),
            round_id: round_id.to_string(),
            attempt_id: "attempt-001".to_string(),
            status: gold_band::domain::RunStatus::Running,
            outcome: None,
            started_at: started_at.clone(),
            finished_at: None,
            manual_check_pending: false,
            runtime_execution_id: None,
            resolved_config: Default::default(),
            uuid: Some(node_uuid.clone()),
        };
        node.resolved_config
            .insert("profileName".to_string(), serde_json::json!("Planner"));
        write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, "plan", "attempt-001"),
            &node,
        )
        .unwrap();

        let bus = gold_band::app::observability::RuntimeLifecycleBus::new();
        let app = app.with_lifecycle_bus(bus.clone());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::MetricsFact(fact) = event {
                seen_for_handler.lock().unwrap().push(fact);
            }
        }));

        build_request_intervention_metrics(
            &app,
            &gold_band::app::AcpLiveEventContext {
                task_id: task_id.to_string(),
                task_uuid: None,
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
                node_id: "plan".to_string(),
                attempt_id: "attempt-001".to_string(),
                outer_node_id: None,
                outer_attempt_id: None,
            },
            "elicit-1",
            RuntimeInterventionKind::ElicitationRequested,
        );

        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 1);
        let fact = &facts[0];
        assert_eq!(
            fact.event_type,
            gold_band::app::observability::LifecycleEventType::InterventionRequested
        );
        assert_eq!(fact.round_index, Some(1));
        assert_eq!(fact.attempt_index, Some(1));
        assert_eq!(fact.attempt_id.as_deref(), Some(node_uuid.as_str()));
        assert_eq!(fact.role_name.as_deref(), Some("Planner"));
        assert_eq!(
            fact.node_id.as_deref(),
            gold_band::app::observability::derive_execution_id(&run_uuid, "round:1:node:plan")
                .as_deref()
        );
        drop(facts);
        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn auto_intervention_metrics_carry_current_dynamic_unit_context() {
        let temp = std::env::temp_dir().join(format!(
            "gold-band-auto-intervention-metrics-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        let repo_root = Utf8PathBuf::from_path_buf(temp.join("repo")).unwrap();
        std::fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let app = App::new(repo_root).with_metrics_collection_enabled(true);
        let task_id = "task-001";
        let run_id = "run-001";
        let round_id = "round-001";
        let outer_node_id = "ai-dynamic";
        let outer_attempt_id = "attempt-001";
        let started_at = "2026-08-11T00:00:00Z".to_string();
        let task_uuid = uuid::Uuid::new_v4().to_string();
        let run_uuid = uuid::Uuid::new_v4().to_string();
        let dynamic_node_uuid = uuid::Uuid::new_v4().to_string();
        write_json(
            &app.paths.run_file(task_id, run_id),
            &RunState {
                version: gold_band::domain::VERSION.to_string(),
                id: run_id.to_string(),
                task_id: task_id.to_string(),
                task_uuid: Some(task_uuid),
                status: gold_band::domain::RunStatus::Paused,
                outcome: None,
                started_at: started_at.clone(),
                updated_at: started_at.clone(),
                workflow_snapshot: "workflow.snapshot.json".to_string(),
                current_round: Some(round_id.to_string()),
                current_node: Some(outer_node_id.to_string()),
                current_attempt: Some(outer_attempt_id.to_string()),
                new_rounds_opened: 0,
                pause_reason: Some(gold_band::domain::PauseReason::WaitingForUserInput),
                uuid: Some(run_uuid),
                last_executed_node: None,
                worktree: None,
                execution: Default::default(),
            },
        )
        .unwrap();
        write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &RoundState {
                version: gold_band::domain::VERSION.to_string(),
                id: round_id.to_string(),
                run_id: run_id.to_string(),
                index: 1,
                status: gold_band::domain::RunStatus::Running,
                outcome: None,
                trigger: gold_band::domain::RoundTrigger::Initial,
                started_at: started_at.clone(),
                trace: Vec::new(),
                uuid: None,
            },
        )
        .unwrap();
        let dynamic_node = serde_json::json!({
            "version": gold_band::domain::VERSION,
            "id": "bootstrap",
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": "Bootstrap",
            "task": "Bootstrap",
            "status": "running",
            "outcome": null,
            "groupId": null,
            "chainId": "bootstrap",
            "depth": 0,
            "dependsOn": [],
            "workspaceId": "workspace-main",
            "provider": "codex-acp",
            "profile": null,
            "permissionMode": null,
            "model": null,
            "sessionMode": "new",
            "continueFromNodeId": null,
            "workflowId": null,
            "workflowSnapshotId": null,
            "childRunId": null,
            "startedAt": started_at,
            "finishedAt": null,
            "uuid": dynamic_node_uuid
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
                "run": {
                    "version": gold_band::domain::VERSION,
                    "id": "dynamic-run-001",
                    "parentRunId": run_id,
                    "parentRoundId": round_id,
                    "parentNodeId": outer_node_id,
                    "parentAttemptId": outer_attempt_id,
                    "status": "running",
                    "outcome": null,
                    "pauseReason": null,
                    "startedAt": "2026-08-11T00:00:00Z",
                    "updatedAt": "2026-08-11T00:00:01Z",
                    "control": {},
                    "allowedWorkflowSnapshots": [],
                    "currentNodeIds": ["bootstrap"]
                },
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
                    "createdAt": "2026-08-11T00:00:00Z",
                    "updatedAt": "2026-08-11T00:00:00Z"
                }],
                "proposals": []
            }),
        )
        .unwrap();

        let bus = gold_band::app::observability::RuntimeLifecycleBus::new();
        let app = app.with_lifecycle_bus(bus.clone());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_handler = seen.clone();
        bus.subscribe_inline(Arc::new(move |event| {
            if let RuntimeLifecycleEvent::MetricsFact(fact) = event {
                seen_for_handler.lock().unwrap().push(fact);
            }
        }));

        build_request_intervention_metrics(
            &app,
            &gold_band::app::AcpLiveEventContext {
                task_id: task_id.to_string(),
                task_uuid: None,
                run_id: run_id.to_string(),
                round_id: round_id.to_string(),
                node_id: "bootstrap".to_string(),
                attempt_id: "attempt-001".to_string(),
                outer_node_id: Some(outer_node_id.to_string()),
                outer_attempt_id: Some(outer_attempt_id.to_string()),
            },
            "permission-1",
            RuntimeInterventionKind::PermissionRequested,
        );

        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 1);
        let fact = &facts[0];
        assert_eq!(
            fact.session_mode,
            gold_band::app::observability::MetricsSessionMode::Auto
        );
        assert_eq!(
            fact.execution_kind,
            gold_band::app::observability::ExecutionKind::OuterRun
        );
        assert_eq!(fact.round_index, Some(1));
        assert_eq!(fact.attempt_index, Some(1));
        assert_eq!(fact.node_id.as_deref(), Some(dynamic_node_uuid.as_str()));
        assert_eq!(fact.role_name.as_deref(), Some("Bootstrap"));
        assert_eq!(
            fact.attempt_id.as_deref(),
            gold_band::app::observability::derive_attempt_id(&dynamic_node_uuid, "attempt-001")
                .as_deref()
        );
        drop(facts);
        let _ = std::fs::remove_dir_all(temp);
    }
}
