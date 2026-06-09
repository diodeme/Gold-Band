import type { AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, ConversationCreateInput, ConversationRunModeVm, ConversationRunVm, ConversationSearchResultVm, ConversationSessionSwitchVm, ConversationSidebarVm, ConversationValidationResultVm, ConversationWorkspaceVm, CreateTaskInput, DesktopFontPreference, DesktopLanguage, DesktopThemePreference, ManagedAgentInput, ProfileInput, RoundSelection, WorkflowDsl } from '../types';
import type { AcpSessionUpdatedEventVm, RuntimeApi } from './client';
import { invokeCommand, isTauriRuntime, toRoundSelectionInput } from './shared';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

const noopUnlisten = () => {};

export const desktopApi: RuntimeApi = {
  async subscribeAcpSessionUpdates(listener) {
    if (!isTauriRuntime()) return noopUnlisten;
    const unlisten: UnlistenFn = await listen<AcpSessionUpdatedEventVm>('gold-band://acp-session-updated', (event) => {
      if (event.payload) listener(event.payload);
    });
    return () => unlisten();
  },
  checkLocalClaude() {
    return invokeCommand('check_local_claude');
  },
  getAppBootstrap() {
    return invokeCommand('get_app_bootstrap');
  },
  getSystemFonts() {
    return invokeCommand('get_system_fonts');
  },
  getAgentRegistry() {
    return invokeCommand('get_agent_registry');
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
  updateProfile(id: string, input: ProfileInput) {
    return invokeCommand('update_profile', { id, input });
  },
  deleteProfile(id: string, force = false) {
    return invokeCommand('delete_profile', { id, force });
  },
  chooseWorkspace() {
    return invokeCommand('choose_workspace');
  },
  selectRecentWorkspace(workspace: string) {
    return invokeCommand('select_recent_workspace', { workspace });
  },
  getTaskDetail(taskId: string) {
    return invokeCommand('get_task_detail', { taskId });
  },
  getWorkflow(taskId: string) {
    return invokeCommand('get_workflow', { taskId });
  },
  createTask(input: CreateTaskInput) {
    return invokeCommand('create_task', { input });
  },
  saveTaskWorkflow(taskId: string, workflow: WorkflowDsl) {
    return invokeCommand('save_task_workflow', { taskId, input: { workflow } });
  },
  getWorkflowTemplates() {
    return invokeCommand('get_workflow_templates');
  },
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl) {
    return invokeCommand('save_workflow_template', { input: { name, workflow } });
  },
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl) {
    return invokeCommand('update_workflow_template', { templateId, input: { workflow } });
  },
  deleteWorkflowTemplate(templateId: string) {
    return invokeCommand('delete_workflow_template', { templateId });
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
  continueRun(taskId: string, runId: string, promptId?: string | null) {
    return invokeCommand('continue_run', { taskId, runId, promptId });
  },
  submitManualCheck(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure') {
    return invokeCommand('submit_manual_check', { taskId, runId, roundId, nodeId, attemptId, outcome });
  },
  retryRun(taskId: string, runId: string) {
    return invokeCommand('retry_run', { taskId, runId });
  },
  killRun(taskId: string, runId: string) {
    return invokeCommand('kill_run', { taskId, runId });
  },
  getLogPage(query) {
    return invokeCommand('get_log_page', { query });
  },
  getAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpSessionQueryInput, _fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand<AcpSessionVm | null>('get_acp_session', { taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  sendAcpPrompt(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, _fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]) {
    return invokeCommand<AcpSessionVm | null>('send_acp_prompt', { taskId, runId, roundId, nodeId, attemptId, prompt, promptId, outerNodeId, outerAttemptId, attachmentPaths });
  },
  setAcpSessionModel(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand<AcpSessionVm | null>('set_acp_session_model', { taskId, runId, roundId, nodeId, attemptId, modelId, outerNodeId, outerAttemptId });
  },
  respondAcpPermission(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, _fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand<AcpSessionVm | null>('respond_acp_permission', { taskId, runId, roundId, nodeId, attemptId, requestId, optionId, outerNodeId, outerAttemptId });
  },
  cancelAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, _fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand<AcpSessionVm | null>('cancel_acp_session', { taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  getAcpRawFrames(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpRawFrameQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('get_acp_raw_frames', { taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId });
  },
  showArtifact(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('show_artifact', { taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId });
  },
  showAttachment(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('show_attachment', { taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId });
  },
  showConversationAttachment(taskId: string, name: string) {
    return invokeCommand('show_conversation_attachment', { taskId, name });
  },
  showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
    return invokeCommand('show_worker_ref', { taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean) {
    return invokeCommand('save_desktop_preferences', { theme, language, font, useLocalClaude });
  },
  saveUpdaterSettings(overrideUrl: string | null) {
    const normalized = overrideUrl?.trim() ? overrideUrl.trim() : null;
    return invokeCommand('save_updater_settings', { overrideUrl: normalized });
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
  getStartupCheckResult() {
    return invokeCommand<import('../types').StartupCheckResult | null>('get_startup_check_result');
  },
  // ── Conversation UI ──
  saveDesktopUiMode(mode) {
    return invokeCommand('save_desktop_ui_mode', { mode });
  },
  getConversationSidebar() {
    return invokeCommand<ConversationSidebarVm>('get_conversation_sidebar');
  },
  getConversationRun(projectId, taskId, runId, selectedSessionKey) {
    return invokeCommand<ConversationRunVm>('get_conversation_run', { projectId, taskId, runId, selectedSessionKey });
  },
  switchConversationSession(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId) {
    return invokeCommand<ConversationSessionSwitchVm>('switch_conversation_session', { taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
  validateConversationCreate(input) {
    return invokeCommand<ConversationValidationResultVm>('validate_conversation_create', { input });
  },
  createConversationRun(input) {
    return invokeCommand<ConversationRunVm>('create_conversation_run', { input });
  },
  rerunConversationTask(projectId, taskId) {
    return invokeCommand<ConversationRunVm>('rerun_conversation_task', { projectId, taskId });
  },
  updateTaskMetadata(projectId, taskId, title, description) {
    return invokeCommand('update_task_metadata', { projectId, taskId, title, description });
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
  addConversationWorkspace() {
    return invokeCommand<ConversationSidebarVm>('add_conversation_workspace');
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
  pickAttachmentFiles() {
    return invokeCommand<Array<{ path: string; name: string; size: number }>>('pick_attachment_files');
  },
  getSupportedAttachmentExtensions() {
    return invokeCommand<string[]>('get_supported_attachment_extensions');
  },
  openInFileManager(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId) {
    return invokeCommand('open_in_file_manager', { taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId });
  },
};
