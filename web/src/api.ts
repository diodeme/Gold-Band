import { getRuntimeApi } from './api/client';
import type { RuntimeApi } from './api/client';

export { isTauriRuntime } from './api/shared';

export function checkLocalClaude() {
  return getRuntimeApi().checkLocalClaude();
}

export function getAppBootstrap() {
  return getRuntimeApi().getAppBootstrap();
}

export function getSystemFonts() {
  return getRuntimeApi().getSystemFonts();
}

export function getAgentRegistry() {
  return getRuntimeApi().getAgentRegistry();
}

export function createAgent(agentType: string, input: Parameters<ReturnType<typeof getRuntimeApi>['createAgent']>[1]) {
  return getRuntimeApi().createAgent(agentType, input);
}

export function updateAgent(agentType: string, input: Parameters<ReturnType<typeof getRuntimeApi>['updateAgent']>[1]) {
  return getRuntimeApi().updateAgent(agentType, input);
}

export function deleteAgent(agentType: string) {
  return getRuntimeApi().deleteAgent(agentType);
}

export function doctorAgent(agentType: string) {
  return getRuntimeApi().doctorAgent(agentType);
}

export function getTaskList() {
  return getRuntimeApi().getTaskList();
}

export function getProfiles() {
  return getRuntimeApi().getProfiles();
}

export function getProfile(id: string) {
  return getRuntimeApi().getProfile(id);
}

export function createProfile(input: Parameters<ReturnType<typeof getRuntimeApi>['createProfile']>[0]) {
  return getRuntimeApi().createProfile(input);
}

export function updateProfile(id: string, input: Parameters<ReturnType<typeof getRuntimeApi>['updateProfile']>[1]) {
  return getRuntimeApi().updateProfile(id, input);
}

export function deleteProfile(id: string, force = false) {
  return getRuntimeApi().deleteProfile(id, force);
}

export function chooseWorkspace() {
  return getRuntimeApi().chooseWorkspace();
}

export function selectRecentWorkspace(workspace: string) {
  return getRuntimeApi().selectRecentWorkspace(workspace);
}

export function getTaskDetail(taskId: string) {
  return getRuntimeApi().getTaskDetail(taskId);
}

export function getWorkflow(taskId: string) {
  return getRuntimeApi().getWorkflow(taskId);
}

export function createTask(input: Parameters<ReturnType<typeof getRuntimeApi>['createTask']>[0]) {
  return getRuntimeApi().createTask(input);
}

export function saveTaskWorkflow(projectId: string | null | undefined, taskId: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['saveTaskWorkflow']>[2]) {
  return getRuntimeApi().saveTaskWorkflow(projectId, taskId, workflow);
}

export function getWorkflowTemplates() {
  return getRuntimeApi().getWorkflowTemplates();
}

export function saveWorkflowTemplate(name: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['saveWorkflowTemplate']>[1]) {
  return getRuntimeApi().saveWorkflowTemplate(name, workflow);
}

export function updateWorkflowTemplate(templateId: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['updateWorkflowTemplate']>[1]) {
  return getRuntimeApi().updateWorkflowTemplate(templateId, workflow);
}

export function deleteWorkflowTemplate(templateId: string) {
  return getRuntimeApi().deleteWorkflowTemplate(templateId);
}

export function getAutoTemplates() {
  return getRuntimeApi().getAutoTemplates();
}

export function saveAutoTemplate(name: string, config: Parameters<ReturnType<typeof getRuntimeApi>['saveAutoTemplate']>[1]) {
  return getRuntimeApi().saveAutoTemplate(name, config);
}

export function updateAutoTemplate(templateId: string, name: string, config: Parameters<ReturnType<typeof getRuntimeApi>['updateAutoTemplate']>[2]) {
  return getRuntimeApi().updateAutoTemplate(templateId, name, config);
}

export function deleteAutoTemplate(templateId: string) {
  return getRuntimeApi().deleteAutoTemplate(templateId);
}

export function replaceAutoTemplates(templates: Parameters<ReturnType<typeof getRuntimeApi>['replaceAutoTemplates']>[0]) {
  return getRuntimeApi().replaceAutoTemplates(templates);
}

export function getRunDetail(taskId: string, runId: string) {
  return getRuntimeApi().getRunDetail(taskId, runId);
}

export function getRoundDetail(taskId: string, runId: string, roundId: string, selection?: Parameters<ReturnType<typeof getRuntimeApi>['getRoundDetail']>[3]) {
  return getRuntimeApi().getRoundDetail(taskId, runId, roundId, selection);
}

export function startRun(taskId: string) {
  return getRuntimeApi().startRun(taskId);
}

export function continueRun(projectId: string | null | undefined, taskId: string, runId: string, promptId?: string | null, prompt?: string | null) {
  return getRuntimeApi().continueRun(projectId, taskId, runId, promptId, prompt);
}

export function pauseRun(taskId: string, runId: string, projectId?: string | null) {
  return getRuntimeApi().pauseRun(taskId, runId, projectId);
}

export function stopActiveSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['stopActiveSession']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().stopActiveSession(projectId, taskId, runId, roundId, nodeId, attemptId, fallback, outerNodeId, outerAttemptId);
}

export function submitManualCheck(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure') {
  return getRuntimeApi().submitManualCheck(projectId, taskId, runId, roundId, nodeId, attemptId, outcome);
}

export function retryRun(taskId: string, runId: string) {
  return getRuntimeApi().retryRun(taskId, runId);
}

export function getLogPage(query: Parameters<ReturnType<typeof getRuntimeApi>['getLogPage']>[0]) {
  return getRuntimeApi().getLogPage(query);
}

export function getAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpSession']>[6], fallback?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpSession']>[7], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, query, fallback, outerNodeId, outerAttemptId);
}

export function subscribeAcpSessionUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeAcpSessionUpdates']>>[0]) {
  return getRuntimeApi().subscribeAcpSessionUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeConversationRunStateUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeConversationRunStateUpdates']>>[0]) {
  return getRuntimeApi().subscribeConversationRunStateUpdates?.(listener) ?? Promise.resolve(() => {});
}

// 干预通知：OS Toast「查看详情」点击后由后端转发导航事件，前端订阅做 deep-link。
export function subscribeInterventionNavigate(listener: Parameters<NonNullable<RuntimeApi['subscribeInterventionNavigate']>>[0]) {
  return getRuntimeApi().subscribeInterventionNavigate?.(listener) ?? Promise.resolve(() => {});
}

export function submitConversationPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['submitConversationPrompt']>[8], outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]) {
  return getRuntimeApi().submitConversationPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, prompt, promptId, fallback, outerNodeId, outerAttemptId, attachmentPaths);
}

export function sendAcpPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['sendAcpPrompt']>[8], outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]) {
  return getRuntimeApi().sendAcpPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, prompt, promptId, fallback, outerNodeId, outerAttemptId, attachmentPaths);
}

export function setAcpSessionModel(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().setAcpSessionModel(projectId, taskId, runId, roundId, nodeId, attemptId, modelId, outerNodeId, outerAttemptId);
}

export function setAcpSessionPermissionMode(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, permissionModeId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().setAcpSessionPermissionMode(projectId, taskId, runId, roundId, nodeId, attemptId, permissionModeId, outerNodeId, outerAttemptId);
}

export function respondAcpPermission(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['respondAcpPermission']>[8], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().respondAcpPermission(projectId, taskId, runId, roundId, nodeId, attemptId, requestId, optionId, fallback, outerNodeId, outerAttemptId);
}

export function respondElicitation(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, elicitationId: string, action: "accept" | "decline", content?: Record<string, unknown> | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().respondElicitation(projectId, taskId, runId, roundId, nodeId, attemptId, elicitationId, action, content, outerNodeId, outerAttemptId);
}

export function cancelAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['cancelAcpSession']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().cancelAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, fallback, outerNodeId, outerAttemptId);
}

export function getAcpRawFrames(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpRawFrames']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpRawFrames(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId);
}

export function showArtifact(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showArtifact(projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId);
}

export function showAttachment(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showAttachment(projectId, taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId);
}

export function showConversationAttachment(projectId: string, taskId: string, name: string) {
  return getRuntimeApi().showConversationAttachment(projectId, taskId, name);
}

export function showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showWorkerRef(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}

export function saveDesktopPreferences(theme: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[0], language: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[1], font: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[2], useLocalClaude: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[3], verboseLogging: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[4]) {
  return getRuntimeApi().saveDesktopPreferences(theme, language, font, useLocalClaude, verboseLogging);
}

export function saveUpdaterSettings(overrideUrl: string | null) {
  return getRuntimeApi().saveUpdaterSettings(overrideUrl);
}

export function updateNotificationAttention(input: Parameters<NonNullable<RuntimeApi['updateNotificationAttention']>>[0]) {
  return getRuntimeApi().updateNotificationAttention?.(input) ?? Promise.resolve();
}

export function getUpdateStatus() {
  return getRuntimeApi().getUpdateStatus();
}

export function markSettingsUpdateSeen(version: string) {
  return getRuntimeApi().markSettingsUpdateSeen(version);
}

export function markSettingsAdvancedUpdateSeen(version: string) {
  return getRuntimeApi().markSettingsAdvancedUpdateSeen(version);
}

export function dismissUpdateAnnouncement(version: string) {
  return getRuntimeApi().dismissUpdateAnnouncement(version);
}

export function checkUpdateManual() {
  return getRuntimeApi().checkUpdateManual();
}

export function downloadAndInstallUpdate() {
  return getRuntimeApi().downloadAndInstallUpdate();
}

export function getMetricsSettings() {
  return getRuntimeApi().getMetricsSettings();
}

export function saveMetricsSettings(enabled: boolean, metricsBaseUrl: string | null, apiKey: string | null) {
  return getRuntimeApi().saveMetricsSettings(enabled, metricsBaseUrl, apiKey);
}
// ── Conversation UI ──
export function saveDesktopUiMode(mode: 'conversation' | 'workbench') {
  return getRuntimeApi().saveDesktopUiMode(mode);
}

export function getConversationSidebar() {
  return getRuntimeApi().getConversationSidebar();
}

export function getConversationRun(projectId: string, taskId: string, runId: string, selectedSessionKey?: string | null) {
  return getRuntimeApi().getConversationRun(projectId, taskId, runId, selectedSessionKey);
}

export function switchConversationSession(projectId: string, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().switchConversationSession(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}

export function validateConversationCreate(input: Parameters<ReturnType<typeof getRuntimeApi>['validateConversationCreate']>[0]) {
  return getRuntimeApi().validateConversationCreate(input);
}

export function createConversationRun(input: Parameters<ReturnType<typeof getRuntimeApi>['createConversationRun']>[0]) {
  return getRuntimeApi().createConversationRun(input);
}

export function rerunConversationTask(projectId: string, taskId: string) {
  return getRuntimeApi().rerunConversationTask(projectId, taskId);
}

export function updateTaskMetadata(projectId: string, taskId: string, title: string, description?: string | null) {
  return getRuntimeApi().updateTaskMetadata(projectId, taskId, title, description);
}

export function deleteConversationTask(projectId: string, taskId: string) {
  return getRuntimeApi().deleteConversationTask(projectId, taskId);
}

export function pinConversation(projectId: string, taskId: string) {
  return getRuntimeApi().pinConversation(projectId, taskId);
}

export function unpinConversation(projectId: string, taskId: string) {
  return getRuntimeApi().unpinConversation(projectId, taskId);
}

export function reorderPinnedConversations(pins: { projectId: string; taskId: string }[]) {
  return getRuntimeApi().reorderPinnedConversations(pins);
}

export function searchConversationTasks(query: string, limit?: number) {
  return getRuntimeApi().searchConversationTasks(query, limit);
}

export function getConversationRunMode(projectId: string) {
  return getRuntimeApi().getConversationRunMode(projectId);
}

export function saveConversationRunMode(projectId: string, settings: Parameters<ReturnType<typeof getRuntimeApi>['saveConversationRunMode']>[1]) {
  return getRuntimeApi().saveConversationRunMode(projectId, settings);
}

export function chooseConversationWorkspace() {
  return getRuntimeApi().chooseConversationWorkspace();
}

export function addConversationWorkspace() {
  return getRuntimeApi().addConversationWorkspace();
}

export function removeConversationWorkspace(projectId: string) {
  return getRuntimeApi().removeConversationWorkspace(projectId);
}

export function syncConversationWorkspace(workspacePath: string) {
  return getRuntimeApi().syncConversationWorkspace(workspacePath);
}

export function saveConversationPreference(key: string, value: unknown) {
  return getRuntimeApi().saveConversationPreference(key, value);
}

export function saveLastConversationWorkspace(projectId: string) {
  return getRuntimeApi().saveLastConversationWorkspace(projectId);
}
// pickAttachmentFiles for file picker in desktop envs
export function pickAttachmentFiles() {
  return getRuntimeApi().pickAttachmentFiles();
}

export function materializeConversationAttachments(files: Parameters<ReturnType<typeof getRuntimeApi>['materializeConversationAttachments']>[0]) {
  return getRuntimeApi().materializeConversationAttachments(files);
}

export function getSupportedAttachmentExtensions() {
  return getRuntimeApi().getSupportedAttachmentExtensions();
}

export function openInFileManager(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId?: string | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().openInFileManager(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}

// ── MCP & SKILL management ──

export function listMcpServers() {
  return getRuntimeApi().listMcpServers();
}

export function addMcpServer(jsonContent: string) {
  return getRuntimeApi().addMcpServer(jsonContent);
}

export function updateMcpServer(id: string, jsonContent: string) {
  return getRuntimeApi().updateMcpServer(id, jsonContent);
}

export function deleteMcpServer(id: string) {
  return getRuntimeApi().deleteMcpServer(id);
}

export function toggleMcpServer(id: string, enabled: boolean) {
  return getRuntimeApi().toggleMcpServer(id, enabled);
}

export function checkMcpServerHealth(id: string) {
  return getRuntimeApi().checkMcpServerHealth(id);
}

export function listSkills() {
  return getRuntimeApi().listSkills();
}

export function listProjectSkills(workspacePath: string) {
  return getRuntimeApi().listProjectSkills(workspacePath);
}

export function readSkill(name: string, source: string, workspacePath?: string | null) {
  return getRuntimeApi().readSkill(name, source, workspacePath);
}

export function writeSkill(name: string, source: string, content: string, workspacePath?: string | null, oldName?: string | null) {
  return getRuntimeApi().writeSkill(name, source, content, workspacePath, oldName);
}

export function deleteSkill(name: string, source: string, workspacePath?: string | null) {
  return getRuntimeApi().deleteSkill(name, source, workspacePath);
}
