import type { AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AppearancePreference, AppBootstrapVm, AppExitRequestVm, AutoTemplate, ConversationAutoConfigVm, ConversationCreateInput, ConversationCreateResultVm, ConversationRunModeVm, ConversationRunVm, ConversationSearchResultVm, ConversationSessionSwitchVm, ConversationSidebarVm, ConversationTaskRowVm, ConversationValidationResultVm, ConversationWorkspaceVm, CreateTaskInput, DesktopLanguage, GitOperationVm, GitStateChangedEventVm, ImportProfilesResult, InterventionNavigateEventVm, ManagedAgentInput, PersonalizationPreference, PreferencesVm, ProfileInput, ResolveAppExitInput, RoundSelection, RunScheduledTaskResultVm, ScheduledNativeNotificationInputVm, ScheduledNotificationEventVm, ScheduledOccurrenceVm, ScheduledTaskDiagnosticsVm, WorkflowDsl, WorkflowModelBindings, WorkspaceFileChangedEventVm } from '../types';
import type { AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm, RuntimeApi, ScheduledOccurrenceUpdatedEventVm, ScheduledTaskUpdatedEventVm } from './client';
import { invokeCommand, isTauriRuntime, toRoundSelectionInput } from './shared';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { convertFileSrc } from '@tauri-apps/api/core';

// ── Metrics Settings ──

export interface MetricsSettingsVm {
    enabled: boolean;
    toggleLocked: boolean;
    metricsBaseUrl: string | null;
    heartbeatEndpoint: string | null;
    nodeMetricsEndpoint: string | null;
    apiKeySet: boolean;
}

const noopUnlisten = () => {};

function emptyWorkflowModelBindings(): WorkflowModelBindings {
  return { definitionRevision: '', bindingRevision: 0, bindings: [] };
}

export function wallpaperAssetUrl(token: string): string {
  return convertFileSrc(token, 'gold-band-wallpaper');
}

function withWallpaperAssetUrls(preferences: PreferencesVm): PreferencesVm {
  return {
    ...preferences,
    wallpapers: {
      ...preferences.wallpapers,
      recentWallpapers: preferences.wallpapers.recentWallpapers.map((wallpaper) => ({
        ...wallpaper,
        imageUrl: wallpaperAssetUrl(wallpaper.imageUrl),
        thumbnailUrl: wallpaperAssetUrl(wallpaper.thumbnailUrl),
      })),
    },
  };
}

function withWallpaperBootstrapAssetUrls(bootstrap: AppBootstrapVm): AppBootstrapVm {
  return { ...bootstrap, preferences: withWallpaperAssetUrls(bootstrap.preferences) };
}

export const desktopApi: RuntimeApi = {
  getGitCapability(projectId) {
    return invokeCommand('get_git_capability', { projectId });
  },
  initializeGitRepository(projectId) {
    return invokeCommand('initialize_git_repository', { projectId });
  },
  getSourceControlSnapshot(projectId, workspacePath) {
    return invokeCommand('get_source_control_snapshot', { projectId, workspacePath });
  },
  getGitHistory(projectId, workspacePath, query) {
    return invokeCommand('get_git_history', { projectId, workspacePath, query });
  },
  getGitCommitDetail(projectId, workspacePath, oid) {
    return invokeCommand('get_git_commit_detail', { projectId, workspacePath, oid });
  },
  getGitCommitReview(projectId, workspacePath, query) {
    return invokeCommand('get_git_commit_review', { projectId, workspacePath, query });
  },
  getGitCommitReachability(projectId, workspacePath, query) {
    return invokeCommand('get_git_commit_reachability', { projectId, workspacePath, query });
  },
  executeGitMutation(projectId, workspacePath, input) {
    return invokeCommand('execute_git_mutation', { projectId, workspacePath, input });
  },
  getGitComparison(projectId, source) {
    return invokeCommand('get_git_comparison', { projectId, source });
  },
  startGitOperation(projectId, workspacePath, input) {
    return invokeCommand('start_git_operation', { projectId, workspacePath, input });
  },
  getGitOperation(operationId) {
    return invokeCommand('get_git_operation', { operationId });
  },
  cancelGitOperation(operationId) {
    return invokeCommand('cancel_git_operation', { operationId });
  },
  startGitStateMonitor(projectId, workspacePath) {
    return invokeCommand('start_git_state_monitor', { projectId, workspacePath });
  },
  stopGitStateMonitor(projectId, workspacePath) {
    return invokeCommand('stop_git_state_monitor', { projectId, workspacePath });
  },
  async subscribeGitOperationUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<GitOperationVm>('gold-band://git-operation-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  async subscribeGitStateChanges(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<GitStateChangedEventVm>('gold-band://git-state-changed', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  getGitHubCapability(projectId, workspacePath) {
    return invokeCommand('get_github_capability', { projectId, workspacePath });
  },
  startGitHubLogin(projectId, workspacePath, host) {
    return invokeCommand('start_github_login', { projectId, workspacePath, host });
  },
  getGitHubOperation(operationId) {
    return invokeCommand('get_github_operation', { operationId });
  },
  cancelGitHubOperation(operationId) {
    return invokeCommand('cancel_github_operation', { operationId });
  },
  async subscribeGitHubOperationUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<import('../types').GitHubOperationVm>('gold-band://github-operation-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  preflightGitHubPullRequest(projectId, workspacePath, input) {
    return invokeCommand('preflight_github_pull_request', { projectId, workspacePath, input });
  },
  startGitHubPullRequestCreate(projectId, workspacePath, input) {
    return invokeCommand('start_github_pull_request_create', { projectId, workspacePath, input });
  },
  listGitHubPullRequests(projectId, workspacePath, host, repository, query) {
    return invokeCommand('list_github_pull_requests', { projectId, workspacePath, host, repository, query });
  },
  getGitHubPullRequest(projectId, workspacePath, host, repository, number) {
    return invokeCommand('get_github_pull_request', { projectId, workspacePath, host, repository, number });
  },
  listGitHubIssues(projectId, workspacePath, host, repository, query) {
    return invokeCommand('list_github_issues', { projectId, workspacePath, host, repository, query });
  },
  getGitHubIssue(projectId, workspacePath, host, repository, number) {
    return invokeCommand('get_github_issue', { projectId, workspacePath, host, repository, number });
  },
  completeMainWindowClose() {
    return invokeCommand('complete_main_window_close');
  },
  resolveAppExit(input: ResolveAppExitInput) {
    return invokeCommand('resolve_app_exit', { input });
  },
  async subscribeAcpSessionUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<AcpSessionUpdatedEventVm>('gold-band://acp-session-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  async subscribeConversationRunStateUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<ConversationRunStateUpdatedEventVm>('gold-band://conversation-run-state-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  async subscribeInterventionNavigate(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    let drain = Promise.resolve();
    const drainPending = () => {
      drain = drain.then(async () => {
        const pending = await desktopApi.takePendingInterventionNavigations();
        pending.forEach(listener);
      }).catch(() => {});
      return drain;
    };
    const unlisten: UnlistenFn = await listen('gold-band://intervention-navigate', () => {
      void drainPending();
    });
    await drainPending();
    return () => unlisten();
  },
  async subscribeAppExitRequested(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<AppExitRequestVm>('gold-band://app-exit-requested', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  takePendingInterventionNavigations() {
    return invokeCommand('take_pending_intervention_navigations');
  },
  async subscribeWorkspaceFileChanges(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<WorkspaceFileChangedEventVm>('gold-band://workspace-file-changed', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  checkLocalClaude() {
    return invokeCommand('check_local_claude');
  },
  async getAppBootstrap() {
    const bootstrap = await invokeCommand<AppBootstrapVm>('get_app_bootstrap');
    return withWallpaperBootstrapAssetUrls(bootstrap);
  },
  getSystemFonts() {
    return invokeCommand('get_system_fonts');
  },
  getAgentRegistry() {
    return invokeCommand('get_agent_registry');
  },
  getAgentCommandCatalog(agentType: string, workspacePath: string) {
    return invokeCommand('get_agent_command_catalog', { agentType, workspacePath });
  },
  createAgent(agentType: string, input: ManagedAgentInput) {
    return invokeCommand('create_agent', { agentType, input });
  },
  updateAgent(agentType: string, input: ManagedAgentInput) {
    return invokeCommand('update_agent', { agentType, input });
  },
  deleteAgent(agentType: string) {
    return invokeCommand('delete_agent', { agentType });
  },
  getAgentBindingUsage(agentType: string) {
    return invokeCommand('get_agent_binding_usage', { agentType });
  },
  doctorAgent(agentType: string) {
    return invokeCommand('doctor_agent', { agentType });
  },
  getTaskList() {
    return invokeCommand('get_task_list');
  },
  getProfiles() {
    return invokeCommand('get_profiles');
  },
  getProfile(id: string) {
    return invokeCommand('get_profile', { id });
  },
  createProfile(input: ProfileInput) {
    return invokeCommand('create_profile', { input });
  },
  importProfilesFromFolder(folderPath: string, dynamicTemplate: boolean) {
    return invokeCommand<ImportProfilesResult>('import_profiles_from_folder', { input: { folderPath, dynamicTemplate } });
  },
  updateProfile(id: string, input: ProfileInput) {
    return invokeCommand('update_profile', { id, input });
  },
  deleteProfile(id: string, force = false) {
    return invokeCommand('delete_profile', { id, force });
  },
  async chooseWorkspace() {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const path = await open({ directory: true });
    if (!path) return null;
    return invokeCommand<AppBootstrapVm>('choose_workspace', { path }).then(withWallpaperBootstrapAssetUrls);
  },
  selectRecentWorkspace(workspace: string) {
    return invokeCommand<AppBootstrapVm>('select_recent_workspace', { workspace }).then(withWallpaperBootstrapAssetUrls);
  },
  removeRecentWorkspace(workspace: string) {
    return invokeCommand<AppBootstrapVm>('remove_recent_workspace', { workspace }).then(withWallpaperBootstrapAssetUrls);
  },
  getTaskDetail(taskId: string) {
    return invokeCommand('get_task_detail', { taskId });
  },
  getWorkflow(taskId: string, projectId?: string | null) {
    return invokeCommand('get_workflow', { projectId, taskId });
  },
  createTask(input: CreateTaskInput) {
    return invokeCommand('create_task', { input });
  },
  saveTaskWorkflow(projectId, taskId, workflow, modelBindings = emptyWorkflowModelBindings()) {
    return invokeCommand('save_task_workflow', { projectId, taskId, input: { workflow, modelBindings } });
  },
  getWorkflowTemplates() {
    return invokeCommand('get_workflow_templates');
  },
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl, modelBindings = emptyWorkflowModelBindings()) {
    return invokeCommand('save_workflow_template', { input: { name, workflow, modelBindings } });
  },
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl, modelBindings = emptyWorkflowModelBindings()) {
    return invokeCommand('update_workflow_template', { templateId, input: { workflow, modelBindings } });
  },
  deleteWorkflowTemplate(templateId: string) {
    return invokeCommand('delete_workflow_template', { templateId });
  },
  getAutoTemplates() {
    return invokeCommand('get_auto_templates');
  },
  saveAutoTemplate(name: string, config: ConversationAutoConfigVm) {
    return invokeCommand('save_auto_template', { input: { name, config } });
  },
  updateAutoTemplate(templateId: string, name: string, config: ConversationAutoConfigVm) {
    return invokeCommand('update_auto_template', { templateId, input: { name, config } });
  },
  deleteAutoTemplate(templateId: string) {
    return invokeCommand('delete_auto_template', { templateId });
  },
  replaceAutoTemplates(templates: AutoTemplate[]) {
    return invokeCommand('replace_auto_templates', { input: { templates } });
  },
  getRunDetail(taskId: string, runId: string) {
    return invokeCommand('get_run_detail', { taskId, runId });
  },
  getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection) {
    return invokeCommand('get_round_detail', { taskId, runId, roundId, selection: toRoundSelectionInput(selection) });
  },
  startRun(taskId: string) {
    return invokeCommand('start_run', { taskId });
  },
  continueRun(projectId, taskId, runId) {
    return invokeCommand('continue_run', { projectId, taskId, runId });
  },
  continueConversationRuntime(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, input, promptId, attachmentPaths) {
    return invokeCommand('continue_conversation_runtime', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, input, promptId, attachmentPaths });
  },
  recoverConversationRuntime(projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision) {
    return invokeCommand('recover_conversation_runtime', { projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision });
  },
  pauseRun(taskId: string, runId: string, projectId?: string | null) {
    return invokeCommand('pause_run', { taskId, runId, projectId });
  },
  stopActiveSession(projectId, taskId, runId, roundId, nodeId, attemptId, _fallback, outerNodeId, outerAttemptId) {
    return invokeCommand('stop_active_session', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  submitManualCheck(projectId, taskId, runId, roundId, nodeId, attemptId, outcome) {
    return invokeCommand('submit_manual_check', { projectId, taskId, runId, roundId, nodeId, attemptId, outcome });
  },
  retryRun(taskId: string, runId: string) {
    return invokeCommand('retry_run', { taskId, runId });
  },
  getLogPage(query) {
    return invokeCommand('get_log_page', { query });
  },
  getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, query, _fallback, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('get_acp_session', { projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  getAcpActivityDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
    return invokeCommand<import('../types').AcpActivityDetailVm>('get_acp_activity_detail', { projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  getAcpToolDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
    return invokeCommand<import('../types').AcpToolDetailVm>('get_acp_tool_detail', { projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  getTurnFileChangeSet(locator, changeSetId) {
    return invokeCommand<import('../types').TurnFileChangeSetVm>('get_turn_file_change_set', { ...locator, changeSetId });
  },
  getFileComparison(locator, changeSetId, changeId) {
    return invokeCommand<import('../types').FileComparisonVm>('get_file_comparison', { ...locator, changeSetId, changeId });
  },
  renewAcpSessionLease(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId) {
    return invokeCommand<number>('renew_acp_session_lease', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  submitConversationPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, input, promptId, _fallback, outerNodeId, outerAttemptId, attachmentPaths) {
    return invokeCommand('submit_conversation_prompt', { projectId, taskId, runId, roundId, nodeId, attemptId, input, promptId, outerNodeId, outerAttemptId, attachmentPaths });
  },
  reorderConversationQueuedPrompts(projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision, orderedItemIds, outerNodeId, outerAttemptId) {
    return invokeCommand('reorder_conversation_queued_prompts', { projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision, orderedItemIds, outerNodeId, outerAttemptId });
  },
  restoreConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId) {
    return invokeCommand('restore_conversation_queued_prompt', { projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId });
  },
  deleteConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId) {
    return invokeCommand('delete_conversation_queued_prompt', { projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId });
  },
  useConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId) {
    return invokeCommand('use_conversation_queued_prompt', { projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId });
  },
  sendAcpPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, prompt, promptId, _fallback, outerNodeId, outerAttemptId, attachmentPaths) {
    return invokeCommand<AcpSessionVm | null>('send_acp_prompt', { projectId, taskId, runId, roundId, nodeId, attemptId, prompt, promptId, outerNodeId, outerAttemptId, attachmentPaths });
  },
  setAcpSessionModel(projectId, taskId, runId, roundId, nodeId, attemptId, modelId, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('set_acp_session_model', { projectId, taskId, runId, roundId, nodeId, attemptId, modelId, outerNodeId, outerAttemptId });
  },
  setAcpSessionPermissionMode(projectId, taskId, runId, roundId, nodeId, attemptId, permissionModeId, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('set_acp_session_permission_mode', { projectId, taskId, runId, roundId, nodeId, attemptId, permissionModeId, outerNodeId, outerAttemptId });
  },
  respondAcpPermission(projectId, taskId, runId, roundId, nodeId, attemptId, requestId, optionId, _fallback, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('respond_acp_permission', { projectId, taskId, runId, roundId, nodeId, attemptId, requestId, optionId, outerNodeId, outerAttemptId });
  },
  respondElicitation(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, elicitationId: string, action: string, content?: Record<string, unknown> | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand<void>('respond_elicitation', { projectId, taskId, runId, roundId, nodeId, attemptId, elicitationId, action, content, outerNodeId, outerAttemptId });
  },
  cancelAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, _fallback, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('cancel_acp_session', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  getAcpRawFrames(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
    return invokeCommand('get_acp_raw_frames', { projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  showArtifact(projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId) {
    return invokeCommand('show_artifact', { projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId });
  },
  showAttachment(projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId) {
    return invokeCommand('show_attachment', { projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId });
  },
  showConversationAttachment(projectId: string, taskId: string, name: string) {
    return invokeCommand('show_conversation_attachment', { projectId, taskId, name });
  },
  showConversationMessageAttachment(projectId: string, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, path: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('show_conversation_message_attachment', { projectId, taskId, runId, roundId, nodeId, attemptId, name, path, outerNodeId, outerAttemptId });
  },
  showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('show_worker_ref', { taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  saveDesktopPreferences(appearance: AppearancePreference, personalization: PersonalizationPreference, language: DesktopLanguage, useLocalClaude: boolean, verboseLogging: boolean) {
    return invokeCommand<PreferencesVm>('save_desktop_preferences', { appearance, personalization, language, useLocalClaude, verboseLogging }).then(withWallpaperAssetUrls);
  },
  saveDesktopAvatar(input) {
    return invokeCommand<PreferencesVm>('save_desktop_avatar', { input }).then(withWallpaperAssetUrls);
  },
  selectRecentDesktopAvatar(kind, avatarId) {
    return invokeCommand<PreferencesVm>('select_recent_desktop_avatar', { kind, avatarId }).then(withWallpaperAssetUrls);
  },
  saveDesktopAvatarShape(kind, shape) {
    return invokeCommand<PreferencesVm>('save_desktop_avatar_shape', { kind, shape }).then(withWallpaperAssetUrls);
  },
  clearDesktopAvatar(kind) {
    return invokeCommand<PreferencesVm>('clear_desktop_avatar', { kind }).then(withWallpaperAssetUrls);
  },
  async importDesktopWallpaper(colorScheme) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const sourcePath = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return null;
    return invokeCommand<PreferencesVm>('import_desktop_wallpaper', {
      input: { sourcePath, colorScheme },
    }).then(withWallpaperAssetUrls);
  },
  selectRecentDesktopWallpaper(colorScheme, wallpaperId) {
    return invokeCommand<PreferencesVm>('select_recent_desktop_wallpaper', { input: { colorScheme, wallpaperId } }).then(withWallpaperAssetUrls);
  },
  saveDesktopWallpaperOpacity(colorScheme, opacityPercent) {
    return invokeCommand<PreferencesVm>('save_desktop_wallpaper_opacity', { input: { colorScheme, opacityPercent } }).then(withWallpaperAssetUrls);
  },
  restoreThemeDesktopWallpaper(colorScheme) {
    return invokeCommand<PreferencesVm>('restore_theme_desktop_wallpaper', { input: { colorScheme } }).then(withWallpaperAssetUrls);
  },
  saveUpdaterSettings(overrideUrl: string | null) {
    const normalized = overrideUrl?.trim() ? overrideUrl.trim() : null;
    return invokeCommand('save_updater_settings', { overrideUrl: normalized });
  },
  updateNotificationAttention(input) {
    return invokeCommand('update_notification_attention', { input });
  },
  getMetricsSettings() {
    return invokeCommand<MetricsSettingsVm>('get_metrics_settings');
  },
  saveMetricsSettings(enabled: boolean, metricsBaseUrl: string | null, apiKey: string | null) {
    return invokeCommand<MetricsSettingsVm>('save_metrics_settings', { enabled, metricsBaseUrl, apiKey });
  },
  recordActivity() {
    return invokeCommand('record_activity');
  },
  getUpdateStatus() {
    return invokeCommand('get_update_status');
  },
  markSettingsUpdateSeen(version: string) {
    return invokeCommand('mark_settings_update_seen', { version });
  },
  markSettingsAdvancedUpdateSeen(version: string) {
    return invokeCommand('mark_settings_advanced_update_seen', { version });
  },
  dismissUpdateAnnouncement(version: string) {
    return invokeCommand('dismiss_update_announcement', { version });
  },
  checkUpdateManual() {
    return invokeCommand('check_update_manual');
  },
  downloadAndInstallUpdate() {
    return invokeCommand('download_and_install_update');
  },
  // ── Conversation UI ──
  saveDesktopUiMode(mode) {
    return invokeCommand('save_desktop_ui_mode', { mode });
  },
  getConversationSidebar() {
    return invokeCommand<ConversationSidebarVm>('get_conversation_sidebar');
  },
  async subscribeScheduledNotifications(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<ScheduledNotificationEventVm>('gold-band://scheduled-notification', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  sendScheduledNativeNotification(input: ScheduledNativeNotificationInputVm) {
    return invokeCommand('send_scheduled_native_notification', { input });
  },
  getScheduledRuntimeSettings() {
    return invokeCommand('get_scheduled_runtime_settings');
  },
  saveScheduledRuntimeSettings(input) {
    return invokeCommand('save_scheduled_runtime_settings', { input });
  },
  async subscribeScheduledTaskUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<ScheduledTaskUpdatedEventVm>('gold-band://scheduled-task-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  async subscribeScheduledOccurrenceUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<ScheduledOccurrenceUpdatedEventVm>('gold-band://scheduled-occurrence-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  listScheduledTasks(projectId) {
    return invokeCommand<import('../types').ScheduledTaskVm[]>('list_scheduled_tasks', { projectId });
  },
  setScheduledTaskEnabled(projectId, scheduledTaskId, enabled) {
    return invokeCommand<import('../types').ScheduledTaskVm>('set_scheduled_task_enabled', { projectId, scheduledTaskId, enabled });
  },
  createScheduledTask(input) {
    return invokeCommand<import('../types').ScheduledTaskVm>('create_scheduled_task', { input });
  },
  getScheduledTask(projectId, scheduledTaskId) {
    return invokeCommand<import('../types').ScheduledTaskEditVm>('get_scheduled_task', { projectId, scheduledTaskId });
  },
  updateScheduledTask(input) {
    return invokeCommand<import('../types').ScheduledTaskEditVm>('update_scheduled_task', { input });
  },
  deleteScheduledTask(projectId, scheduledTaskId) {
    return invokeCommand<void>('delete_scheduled_task', { projectId, scheduledTaskId });
  },
  listScheduledTaskOccurrences(projectId, scheduledTaskId, cursor, status) {
    return invokeCommand<import('../types').ScheduledOccurrencePageVm>('list_scheduled_task_occurrences', { projectId, scheduledTaskId, cursor, status });
  },
  getScheduledTaskDiagnostics(projectId, scheduledTaskId) {
    return invokeCommand<ScheduledTaskDiagnosticsVm>('get_scheduled_task_diagnostics', { projectId, scheduledTaskId });
  },
  runScheduledTaskNow(projectId, scheduledTaskId) {
    return invokeCommand<RunScheduledTaskResultVm>('run_scheduled_task_now', { projectId, scheduledTaskId });
  },
  setAcpSessionConfigOption(projectId, taskId, runId, roundId, nodeId, attemptId, optionId, optionValue, outerNodeId, outerAttemptId) {
    return invokeCommand<AcpSessionVm | null>('set_acp_session_config_option', { projectId, taskId, runId, roundId, nodeId, attemptId, optionId, optionValue, outerNodeId, outerAttemptId });
  },
  getConversationWorkspaces() {
    return invokeCommand<ConversationWorkspaceVm[]>('get_conversation_workspaces');
  },
  getConversationRun(projectId, taskId, runId, selectedSessionKey) {
    return invokeCommand<ConversationRunVm>('get_conversation_run', { projectId, taskId, runId, selectedSessionKey });
  },
  switchConversationSession(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId) {
    return invokeCommand<ConversationSessionSwitchVm>('switch_conversation_session', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  validateConversationCreate(input) {
    return invokeCommand<ConversationValidationResultVm>('validate_conversation_create', { input });
  },
  createConversationRun(input) {
    return invokeCommand<ConversationCreateResultVm>('create_conversation_run', { input });
  },
  rerunConversationTask(projectId, taskId) {
    return invokeCommand<ConversationRunVm>('rerun_conversation_task', { projectId, taskId });
  },
  updateTaskMetadata(projectId, taskId, title, description) {
    return invokeCommand<ConversationTaskRowVm>('update_task_metadata', { projectId, taskId, title, description });
  },
  deleteConversationTask(projectId, taskId) {
    return invokeCommand<ConversationSidebarVm>('delete_conversation_task', { projectId, taskId });
  },
  pinConversation(projectId, taskId) {
    return invokeCommand<ConversationSidebarVm>('pin_conversation', { projectId, taskId });
  },
  unpinConversation(projectId, taskId) {
    return invokeCommand<ConversationSidebarVm>('unpin_conversation', { projectId, taskId });
  },
  reorderPinnedConversations(pins) {
    return invokeCommand<ConversationSidebarVm>('reorder_pinned_conversations', { ordered: pins.map((p) => ({ project_id: p.projectId, task_id: p.taskId, order: 0 })) });
  },
  searchConversationTasks(query, limit) {
    return invokeCommand<ConversationSearchResultVm[]>('search_conversation_tasks', { query, limit });
  },
  getConversationRunMode(projectId) {
    return invokeCommand<ConversationRunModeVm | null>('get_conversation_run_mode', { projectId });
  },
  saveConversationRunMode(projectId, settings) {
    return invokeCommand('save_conversation_run_mode', { projectId, settings });
  },
  chooseConversationWorkspace() {
    return invokeCommand<ConversationWorkspaceVm>('choose_conversation_workspace');
  },
  async addConversationWorkspace() {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const path = await open({ directory: true });
    if (!path) {
      throw new Error('workspace.cancelled');
    }
    return invokeCommand<ConversationSidebarVm>('add_conversation_workspace', { path });
  },
  removeConversationWorkspace(projectId) {
    return invokeCommand<ConversationSidebarVm>('remove_conversation_workspace', { projectId });
  },
  syncConversationWorkspace(workspacePath) {
    return invokeCommand<ConversationSidebarVm>('sync_conversation_workspace', { workspacePath });
  },
  saveConversationPreference(key, value) {
    return invokeCommand('save_conversation_preference', { key, value });
  },
  saveLastConversationWorkspace(projectId) {
    return invokeCommand('save_last_conversation_workspace', { projectId });
  },
  listWorkspaceDirectory(projectId, relativePath) {
    return invokeCommand('list_workspace_directory', { input: { projectId, relativePath } });
  },
  openWorkspacePathInFileManager(projectId, relativePath = '') {
    return invokeCommand('open_workspace_path_in_file_manager', { input: { projectId, relativePath } });
  },
  listConversationDirectory(input) {
    return invokeCommand('list_conversation_directory', { input });
  },
  openConversationDirectoryPathInFileManager(input) {
    return invokeCommand('open_conversation_directory_path_in_file_manager', { input });
  },
  readConversationDirectoryFile(input) { return invokeCommand('read_conversation_directory_file', { input }); },
  searchWorkspaceFiles(projectId, query, requestId, limit) {
    return invokeCommand('search_workspace_files', { input: { projectId, query, requestId, limit } });
  },
  resolveWorkspaceFileLink(projectId, rawHref, baseCanonicalPath = null) {
    return invokeCommand('resolve_workspace_file_link', { input: { projectId, rawHref, baseCanonicalPath } });
  },
  readFileResource(projectId, canonicalPath, externalAccessToken = null, preferSource = false) {
    return invokeCommand('read_file_resource', { input: { projectId, canonicalPath, externalAccessToken, preferSource } });
  },
  resolveMarkdownImage(input) {
    return invokeCommand('resolve_markdown_image', { input });
  },
  writeFileResource(input) {
    return invokeCommand('write_file_resource', { input });
  },
  releaseWorkspaceFilePreview(token) {
    return invokeCommand('release_workspace_file_preview', { input: { token } });
  },
  renewExternalFileAccess(token) {
    return invokeCommand('renew_external_file_access', { input: { token } });
  },
  releaseExternalFileAccess(token) {
    return invokeCommand('release_external_file_access', { input: { token } });
  },
  startWorkspaceFileWatch(projectId) {
    return invokeCommand('start_workspace_file_watch', { input: { projectId } });
  },
  stopWorkspaceFileWatch(projectId) {
    return invokeCommand('stop_workspace_file_watch', { input: { projectId } });
  },
  workspaceFilePreviewUrl(token, staticFrame = false) {
    return convertFileSrc(staticFrame ? `${token}/static` : token, 'gold-band-preview');
  },
  async openExternalUrl(url) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  },
  async openFileWithSystemApp(path) {
    const { openPath } = await import('@tauri-apps/plugin-opener');
    await openPath(path);
  },
  async pickAttachmentFiles() {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const result = await open({ multiple: true });
    if (!result) return [];
    const paths = Array.isArray(result) ? result : [result];
    const files = await invokeCommand<import('./client').AttachmentFileRef[]>('stat_attachment_files', { paths });
    return files.map((file) => ({
      ...file,
      previewUrl: file.previewUrl ? convertFileSrc(file.previewUrl, 'gold-band-preview') : null,
      contentUrl: file.contentUrl ? convertFileSrc(file.contentUrl, 'gold-band-preview') : null,
    }));
  },
  async statAttachmentFiles(paths) {
    const files = await invokeCommand<import('./client').AttachmentFileRef[]>('stat_attachment_files', { paths });
    return files.map((file) => ({
      ...file,
      previewUrl: file.previewUrl ? convertFileSrc(file.previewUrl, 'gold-band-preview') : null,
      contentUrl: file.contentUrl ? convertFileSrc(file.contentUrl, 'gold-band-preview') : null,
    }));
  },
  materializeConversationAttachments(files) {
    return invokeCommand('materialize_conversation_attachments', { input: { files } });
  },
  getSupportedAttachmentExtensions() {
    return invokeCommand<string[]>('get_supported_attachment_extensions');
  },
  openInFileManager(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId) {
    return invokeCommand('open_in_file_manager', { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  // ── MCP & SKILL management ──
  listMcpServers() {
    return invokeCommand('list_mcp_servers');
  },
  addMcpServer(jsonContent: string) {
    return invokeCommand('add_mcp_server', { jsonContent });
  },
  updateMcpServer(id: string, jsonContent: string) {
    return invokeCommand('update_mcp_server', { id, jsonContent });
  },
  deleteMcpServer(id: string) {
    return invokeCommand('delete_mcp_server', { id });
  },
  toggleMcpServer(id: string, enabled: boolean) {
    return invokeCommand('toggle_mcp_server', { id, enabled });
  },
  checkMcpServerHealth(id: string) {
    return invokeCommand('check_mcp_server_health', { id });
  },
  listMcpTools(id: string) {
    return invokeCommand('list_mcp_tools', { id });
  },
  listSkills() {
    return invokeCommand('list_skills');
  },
  listProjectSkills(workspacePath: string) {
    return invokeCommand('list_project_skills', { workspacePath });
  },
  readSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null) {
    return invokeCommand('read_skill', { name, source, workspacePath, directoryPath });
  },
  writeSkill(
    name: string,
    source: string,
    content: string,
    workspacePath?: string | null,
    oldName?: string | null,
    directoryPath?: string | null,
    syncTargets?: string[] | null,
  ) {
    return invokeCommand('write_skill', { name, source, content, workspacePath, oldName, directoryPath, syncTargets });
  },
  deleteSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null) {
    return invokeCommand('delete_skill', { name, source, workspacePath, directoryPath });
  },
  updateSkillSyncTargets(
    name: string,
    source: string,
    workspacePath: string | null | undefined,
    directoryPath: string,
    syncTargets: string[],
  ) {
    return invokeCommand('update_skill_sync_targets', { name, source, workspacePath, directoryPath, syncTargets });
  },
  getSkillSyncStatus(name: string, directoryPath: string, workspacePath?: string | null) {
    return invokeCommand<import('../types').SyncStatusEntryVm[]>('get_skill_sync_status', { name, directoryPath, workspacePath });
  },
  checkSkillNameConflict(
    name: string,
    source: string,
    workspacePath?: string | null,
    oldName?: string | null,
    directoryPath?: string | null,
    syncTargets?: string[] | null,
  ) {
    return invokeCommand<string[]>('check_skill_name_conflict', {
      name,
      source,
      workspacePath,
      oldName,
      directoryPath,
      syncTargets,
    });
  },
  submitFeedback(input: import('../types').FeedbackInput) {
    return invokeCommand('submit_feedback', { input });
  },
  previewFeedbackSessionArchive(projectId, taskId) {
    return invokeCommand('preview_feedback_session_archive', { projectId, taskId });
  },
};
