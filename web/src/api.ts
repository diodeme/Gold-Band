import { getRuntimeApi } from './api/client';

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

export function saveTaskWorkflow(taskId: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['saveTaskWorkflow']>[1]) {
  return getRuntimeApi().saveTaskWorkflow(taskId, workflow);
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

export function getRunDetail(taskId: string, runId: string) {
  return getRuntimeApi().getRunDetail(taskId, runId);
}

export function getRoundDetail(taskId: string, runId: string, roundId: string, selection?: Parameters<ReturnType<typeof getRuntimeApi>['getRoundDetail']>[3]) {
  return getRuntimeApi().getRoundDetail(taskId, runId, roundId, selection);
}

export function startRun(taskId: string) {
  return getRuntimeApi().startRun(taskId);
}

export function continueRun(taskId: string, runId: string, promptId?: string | null) {
  return getRuntimeApi().continueRun(taskId, runId, promptId);
}

export function submitManualCheck(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure') {
  return getRuntimeApi().submitManualCheck(taskId, runId, roundId, nodeId, attemptId, outcome);
}

export function retryRun(taskId: string, runId: string) {
  return getRuntimeApi().retryRun(taskId, runId);
}

export function killRun(taskId: string, runId: string) {
  return getRuntimeApi().killRun(taskId, runId);
}

export function getLogPage(query: Parameters<ReturnType<typeof getRuntimeApi>['getLogPage']>[0]) {
  return getRuntimeApi().getLogPage(query);
}

export function getAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpSession']>[5], fallback?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpSession']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpSession(taskId, runId, roundId, nodeId, attemptId, query, fallback, outerNodeId, outerAttemptId);
}

export function sendAcpPrompt(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['sendAcpPrompt']>[7], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().sendAcpPrompt(taskId, runId, roundId, nodeId, attemptId, prompt, promptId, fallback, outerNodeId, outerAttemptId);
}

export function respondAcpPermission(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['respondAcpPermission']>[7], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().respondAcpPermission(taskId, runId, roundId, nodeId, attemptId, requestId, optionId, fallback, outerNodeId, outerAttemptId);
}

export function cancelAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['cancelAcpSession']>[5], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().cancelAcpSession(taskId, runId, roundId, nodeId, attemptId, fallback, outerNodeId, outerAttemptId);
}

export function getAcpRawFrames(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: Parameters<ReturnType<typeof getRuntimeApi>['getAcpRawFrames']>[5], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpRawFrames(taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId);
}

export function showArtifact(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showArtifact(taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId);
}

export function showAttachment(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showAttachment(taskId, runId, roundId, nodeId, attemptId, name, outerNodeId, outerAttemptId);
}

export function showConversationAttachment(taskId: string, name: string) {
  return getRuntimeApi().showConversationAttachment(taskId, name);
}

export function showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showWorkerRef(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}

export function saveDesktopPreferences(theme: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[0], language: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[1], font: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[2], useLocalClaude: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[3]) {
  return getRuntimeApi().saveDesktopPreferences(theme, language, font, useLocalClaude);
}

export function saveUpdaterSettings(overrideUrl: string | null) {
  return getRuntimeApi().saveUpdaterSettings(overrideUrl);
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

export function switchConversationSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().switchConversationSession(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
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
// pickAttachmentFiles for file picker in desktop envs
export function pickAttachmentFiles() {
  return getRuntimeApi().pickAttachmentFiles();
}

export function openInFileManager(taskId: string, runId: string, roundId: string, nodeId: string, attemptId?: string | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().openInFileManager(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}
