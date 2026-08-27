import { getRuntimeApi } from './api/client';
import type { RuntimeApi } from './api/client';
import type { ResolvedColorScheme } from './types';

export { isTauriRuntime } from './api/shared';

export function checkLocalClaude() {
  return getRuntimeApi().checkLocalClaude();
}

export function getAppBootstrap() {
  return getRuntimeApi().getAppBootstrap();
}

export function completeMainWindowClose() {
  return getRuntimeApi().completeMainWindowClose();
}

export function resolveAppExit(input: Parameters<ReturnType<typeof getRuntimeApi>['resolveAppExit']>[0]) {
  return getRuntimeApi().resolveAppExit(input);
}

export function getSystemFonts() {
  return getRuntimeApi().getSystemFonts();
}

export function getAgentRegistry() {
  return getRuntimeApi().getAgentRegistry();
}

export function getAgentCommandCatalog(agentType: string, workspacePath: string) {
  return getRuntimeApi().getAgentCommandCatalog(agentType, workspacePath);
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

export function getAgentBindingUsage(agentType: string) {
  return getRuntimeApi().getAgentBindingUsage(agentType);
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

export function importProfilesFromFolder(folderPath: string, dynamicTemplate: boolean) {
  return getRuntimeApi().importProfilesFromFolder(folderPath, dynamicTemplate);
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

export function removeRecentWorkspace(workspace: string) {
  return getRuntimeApi().removeRecentWorkspace(workspace);
}

export function getTaskDetail(taskId: string) {
  return getRuntimeApi().getTaskDetail(taskId);
}

export function getWorkflow(taskId: string, projectId?: string | null) {
  return getRuntimeApi().getWorkflow(taskId, projectId);
}

export function createTask(input: Parameters<ReturnType<typeof getRuntimeApi>['createTask']>[0]) {
  return getRuntimeApi().createTask(input);
}

export function saveTaskWorkflow(projectId: string | null | undefined, taskId: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['saveTaskWorkflow']>[2], modelBindings?: Parameters<ReturnType<typeof getRuntimeApi>['saveTaskWorkflow']>[3]) {
  return getRuntimeApi().saveTaskWorkflow(projectId, taskId, workflow, modelBindings);
}

export function getWorkflowTemplates() {
  return getRuntimeApi().getWorkflowTemplates();
}

export function saveWorkflowTemplate(name: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['saveWorkflowTemplate']>[1], modelBindings?: Parameters<ReturnType<typeof getRuntimeApi>['saveWorkflowTemplate']>[2]) {
  return getRuntimeApi().saveWorkflowTemplate(name, workflow, modelBindings);
}

export function updateWorkflowTemplate(templateId: string, workflow: Parameters<ReturnType<typeof getRuntimeApi>['updateWorkflowTemplate']>[1], modelBindings?: Parameters<ReturnType<typeof getRuntimeApi>['updateWorkflowTemplate']>[2]) {
  return getRuntimeApi().updateWorkflowTemplate(templateId, workflow, modelBindings);
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

export function continueRun(projectId: string | null | undefined, taskId: string, runId: string) {
  return getRuntimeApi().continueRun(projectId, taskId, runId);
}

export function continueConversationRuntime(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null, input?: import('./types').ConversationPromptInput, promptId?: string | null, attachmentPaths?: string[]) {
  return getRuntimeApi().continueConversationRuntime(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, input, promptId, attachmentPaths);
}

export function recoverConversationRuntime(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, expectedRevision: number) {
  return getRuntimeApi().recoverConversationRuntime(projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision);
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

export function getGitCapability(projectId?: string | null) {
  return getRuntimeApi().getGitCapability(projectId);
}

export function initializeGitRepository(projectId?: string | null) {
  return getRuntimeApi().initializeGitRepository(projectId);
}

export function getSourceControlSnapshot(projectId: string, workspacePath?: string | null) {
  return getRuntimeApi().getSourceControlSnapshot(projectId, workspacePath);
}

export function getGitBranchPickerSnapshot(projectId: string, workspacePath?: string | null) {
  return getRuntimeApi().getGitBranchPickerSnapshot(projectId, workspacePath);
}

export function changeGitBranch(projectId: string, workspacePath: string | null | undefined, input: Parameters<ReturnType<typeof getRuntimeApi>['changeGitBranch']>[2]) {
  return getRuntimeApi().changeGitBranch(projectId, workspacePath, input);
}

export function getGitHistory(projectId: string, workspacePath: string | null | undefined, query: Parameters<ReturnType<typeof getRuntimeApi>['getGitHistory']>[2]) {
  return getRuntimeApi().getGitHistory(projectId, workspacePath, query);
}

export function getGitCommitDetail(projectId: string, workspacePath: string | null | undefined, oid: string) {
  return getRuntimeApi().getGitCommitDetail(projectId, workspacePath, oid);
}

export function getGitCommitReview(projectId: string, workspacePath: string | null | undefined, query: Parameters<ReturnType<typeof getRuntimeApi>['getGitCommitReview']>[2]) {
  return getRuntimeApi().getGitCommitReview(projectId, workspacePath, query);
}

export function getGitCommitReachability(projectId: string, workspacePath: string | null | undefined, query: Parameters<ReturnType<typeof getRuntimeApi>['getGitCommitReachability']>[2]) {
  return getRuntimeApi().getGitCommitReachability(projectId, workspacePath, query);
}

export function executeGitMutation(projectId: string, workspacePath: string | null | undefined, input: Parameters<ReturnType<typeof getRuntimeApi>['executeGitMutation']>[2]) {
  return getRuntimeApi().executeGitMutation(projectId, workspacePath, input);
}

export function getGitComparison(projectId: string, source: Parameters<ReturnType<typeof getRuntimeApi>['getGitComparison']>[1]) {
  return getRuntimeApi().getGitComparison(projectId, source);
}

export function startGitOperation(projectId: string, workspacePath: string | null | undefined, input: Parameters<ReturnType<typeof getRuntimeApi>['startGitOperation']>[2]) {
  return getRuntimeApi().startGitOperation(projectId, workspacePath, input);
}

export function getGitOperation(operationId: string) {
  return getRuntimeApi().getGitOperation(operationId);
}

export function cancelGitOperation(operationId: string) {
  return getRuntimeApi().cancelGitOperation(operationId);
}

export function startGitStateMonitor(projectId: string, workspacePath: string | null | undefined) {
  return getRuntimeApi().startGitStateMonitor(projectId, workspacePath);
}

export function stopGitStateMonitor(projectId: string, workspacePath: string | null | undefined) {
  return getRuntimeApi().stopGitStateMonitor(projectId, workspacePath);
}

export function subscribeGitOperationUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeGitOperationUpdates']>>[0]) {
  return getRuntimeApi().subscribeGitOperationUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeGitStateChanges(listener: Parameters<NonNullable<RuntimeApi['subscribeGitStateChanges']>>[0]) {
  return getRuntimeApi().subscribeGitStateChanges?.(listener) ?? Promise.resolve(() => {});
}

export function getGitHubCapability(projectId: string, workspacePath?: string | null) {
  return getRuntimeApi().getGitHubCapability(projectId, workspacePath);
}

export function startGitHubLogin(projectId: string, workspacePath: string | null | undefined, host: string) {
  return getRuntimeApi().startGitHubLogin(projectId, workspacePath, host);
}

export function getGitHubOperation(operationId: string) {
  return getRuntimeApi().getGitHubOperation(operationId);
}

export function cancelGitHubOperation(operationId: string) {
  return getRuntimeApi().cancelGitHubOperation(operationId);
}

export function subscribeGitHubOperationUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeGitHubOperationUpdates']>>[0]) {
  return getRuntimeApi().subscribeGitHubOperationUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function preflightGitHubPullRequest(projectId: string, workspacePath: string | null | undefined, input: Parameters<ReturnType<typeof getRuntimeApi>['preflightGitHubPullRequest']>[2]) {
  return getRuntimeApi().preflightGitHubPullRequest(projectId, workspacePath, input);
}

export function startGitHubPullRequestCreate(projectId: string, workspacePath: string | null | undefined, input: Parameters<ReturnType<typeof getRuntimeApi>['startGitHubPullRequestCreate']>[2]) {
  return getRuntimeApi().startGitHubPullRequestCreate(projectId, workspacePath, input);
}

export function listGitHubPullRequests(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, query: Parameters<ReturnType<typeof getRuntimeApi>['listGitHubPullRequests']>[4]) {
  return getRuntimeApi().listGitHubPullRequests(projectId, workspacePath, host, repository, query);
}

export function getGitHubPullRequest(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, number: number) {
  return getRuntimeApi().getGitHubPullRequest(projectId, workspacePath, host, repository, number);
}

export function listGitHubIssues(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, query: Parameters<ReturnType<typeof getRuntimeApi>['listGitHubIssues']>[4]) {
  return getRuntimeApi().listGitHubIssues(projectId, workspacePath, host, repository, query);
}

export function getGitHubIssue(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, number: number) {
  return getRuntimeApi().getGitHubIssue(projectId, workspacePath, host, repository, number);
}

export function getAcpActivityDetail(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query: Parameters<ReturnType<typeof getRuntimeApi>['getAcpActivityDetail']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpActivityDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId);
}

export function getAcpToolDetail(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query: Parameters<ReturnType<typeof getRuntimeApi>['getAcpToolDetail']>[6], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().getAcpToolDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId);
}

export function getTurnFileChangeSet(locator: Parameters<ReturnType<typeof getRuntimeApi>['getTurnFileChangeSet']>[0], changeSetId: string) {
  return getRuntimeApi().getTurnFileChangeSet(locator, changeSetId);
}

export function getFileComparison(locator: Parameters<ReturnType<typeof getRuntimeApi>['getFileComparison']>[0], changeSetId: string, changeId: string) {
  return getRuntimeApi().getFileComparison(locator, changeSetId, changeId);
}

export function subscribeAcpSessionUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeAcpSessionUpdates']>>[0]) {
  return getRuntimeApi().subscribeAcpSessionUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeConversationRunStateUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeConversationRunStateUpdates']>>[0]) {
  return getRuntimeApi().subscribeConversationRunStateUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeConversationTerminalResultUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeConversationTerminalResultUpdates']>>[0]) {
  return getRuntimeApi().subscribeConversationTerminalResultUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeScheduledTaskUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeScheduledTaskUpdates']>>[0]) {
  return getRuntimeApi().subscribeScheduledTaskUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeScheduledOccurrenceUpdates(listener: Parameters<NonNullable<RuntimeApi['subscribeScheduledOccurrenceUpdates']>>[0]) {
  return getRuntimeApi().subscribeScheduledOccurrenceUpdates?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeScheduledNotifications(listener: Parameters<NonNullable<RuntimeApi['subscribeScheduledNotifications']>>[0]) {
  return getRuntimeApi().subscribeScheduledNotifications?.(listener) ?? Promise.resolve(() => {});
}

export function sendScheduledNativeNotification(input: Parameters<RuntimeApi['sendScheduledNativeNotification']>[0]) {
  return getRuntimeApi().sendScheduledNativeNotification(input);
}

export function getScheduledRuntimeSettings() {
  return getRuntimeApi().getScheduledRuntimeSettings();
}

export function saveScheduledRuntimeSettings(input: Parameters<RuntimeApi['saveScheduledRuntimeSettings']>[0]) {
  return getRuntimeApi().saveScheduledRuntimeSettings(input);
}

// 干预通知：OS Toast「查看详情」点击后由后端转发导航事件，前端订阅做 deep-link。
export function subscribeInterventionNavigate(listener: Parameters<NonNullable<RuntimeApi['subscribeInterventionNavigate']>>[0]) {
  return getRuntimeApi().subscribeInterventionNavigate?.(listener) ?? Promise.resolve(() => {});
}

export function subscribeAppExitRequested(listener: Parameters<NonNullable<RuntimeApi['subscribeAppExitRequested']>>[0]) {
  return getRuntimeApi().subscribeAppExitRequested?.(listener) ?? Promise.resolve(() => {});
}

export function submitConversationPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, input: import('./types').ConversationPromptInput, promptId?: string | null, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['submitConversationPrompt']>[8], outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]) {
  return getRuntimeApi().submitConversationPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, input, promptId, fallback, outerNodeId, outerAttemptId, attachmentPaths);
}

export function reorderConversationQueuedPrompts(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, expectedRevision: number, orderedItemIds: string[], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().reorderConversationQueuedPrompts(projectId, taskId, runId, roundId, nodeId, attemptId, expectedRevision, orderedItemIds, outerNodeId, outerAttemptId);
}

export function restoreConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().restoreConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId);
}

export function deleteConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().deleteConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId);
}

export function useConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().useConversationQueuedPrompt(projectId, taskId, runId, roundId, nodeId, attemptId, itemId, outerNodeId, outerAttemptId);
}

export function setAcpSessionModel(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().setAcpSessionModel(projectId, taskId, runId, roundId, nodeId, attemptId, modelId, outerNodeId, outerAttemptId);
}

export function setAcpSessionPermissionMode(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, permissionModeId: string | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().setAcpSessionPermissionMode(projectId, taskId, runId, roundId, nodeId, attemptId, permissionModeId, outerNodeId, outerAttemptId);
}

export function respondAcpPermission(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: Parameters<ReturnType<typeof getRuntimeApi>['respondAcpPermission']>[8], outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().respondAcpPermission(projectId, taskId, runId, roundId, nodeId, attemptId, requestId, optionId, fallback, outerNodeId, outerAttemptId);
}

export function respondElicitation(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, elicitationId: string, action: "accept" | "decline", content?: Record<string, unknown> | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().respondElicitation(projectId, taskId, runId, roundId, nodeId, attemptId, elicitationId, action, content, outerNodeId, outerAttemptId);
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

export function showConversationMessageAttachment(projectId: string, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, path: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showConversationMessageAttachment(projectId, taskId, runId, roundId, nodeId, attemptId, name, path, outerNodeId, outerAttemptId);
}

export function showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().showWorkerRef(taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
}

export function saveDesktopPreferences(appearance: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[0], personalization: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[1], language: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[2], useLocalClaude: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[3], verboseLogging: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopPreferences']>[4]) {
  return getRuntimeApi().saveDesktopPreferences(appearance, personalization, language, useLocalClaude, verboseLogging);
}

export function saveDesktopAvatar(input: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopAvatar']>[0]) {
  return getRuntimeApi().saveDesktopAvatar(input);
}

export function selectRecentDesktopAvatar(kind: Parameters<ReturnType<typeof getRuntimeApi>['selectRecentDesktopAvatar']>[0], avatarId: string) {
  return getRuntimeApi().selectRecentDesktopAvatar(kind, avatarId);
}

export function saveDesktopAvatarShape(kind: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopAvatarShape']>[0], shape: Parameters<ReturnType<typeof getRuntimeApi>['saveDesktopAvatarShape']>[1]) {
  return getRuntimeApi().saveDesktopAvatarShape(kind, shape);
}

export function clearDesktopAvatar(kind: Parameters<ReturnType<typeof getRuntimeApi>['clearDesktopAvatar']>[0]) {
  return getRuntimeApi().clearDesktopAvatar(kind);
}

export function importDesktopWallpaper(colorScheme: ResolvedColorScheme) {
  return getRuntimeApi().importDesktopWallpaper(colorScheme);
}

export function selectRecentDesktopWallpaper(colorScheme: ResolvedColorScheme, wallpaperId: string) {
  return getRuntimeApi().selectRecentDesktopWallpaper(colorScheme, wallpaperId);
}

export function saveDesktopWallpaperOpacity(colorScheme: ResolvedColorScheme, opacityPercent: number) {
  return getRuntimeApi().saveDesktopWallpaperOpacity(colorScheme, opacityPercent);
}

export function restoreThemeDesktopWallpaper(colorScheme: ResolvedColorScheme) {
  return getRuntimeApi().restoreThemeDesktopWallpaper(colorScheme);
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

export function recordActivity() {
  return getRuntimeApi().recordActivity();
}

export function reportFrontendError(input: import('./api/client').FrontendErrorReportInput) {
  return getRuntimeApi().reportFrontendError(input);
}
// ── Conversation UI ──
export function saveDesktopUiMode(mode: 'conversation' | 'workbench') {
  return getRuntimeApi().saveDesktopUiMode(mode);
}

export function getConversationSidebar() {
  return getRuntimeApi().getConversationSidebar();
}

export function acknowledgeConversationTerminalResult(projectId: string, taskId: string, eventId: string) {
  return getRuntimeApi().acknowledgeConversationTerminalResult(projectId, taskId, eventId);
}

export function setAcpSessionConfigOption(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, optionId: string, optionValue: string | null, outerNodeId?: string | null, outerAttemptId?: string | null) {
  return getRuntimeApi().setAcpSessionConfigOption(projectId, taskId, runId, roundId, nodeId, attemptId, optionId, optionValue, outerNodeId, outerAttemptId);
}

export function getConversationWorkspaces() {
  return getRuntimeApi().getConversationWorkspaces();
}

export function listScheduledTasks(projectId?: string | null) {
  return getRuntimeApi().listScheduledTasks(projectId);
}

export function setScheduledTaskEnabled(projectId: string | null | undefined, scheduledTaskId: string, enabled: boolean) {
  return getRuntimeApi().setScheduledTaskEnabled(projectId, scheduledTaskId, enabled);
}

export function createScheduledTask(input: Parameters<ReturnType<typeof getRuntimeApi>['createScheduledTask']>[0]) {
  return getRuntimeApi().createScheduledTask(input);
}

export function getScheduledTask(projectId: string, scheduledTaskId: string) {
  return getRuntimeApi().getScheduledTask(projectId, scheduledTaskId);
}

export function updateScheduledTask(input: Parameters<ReturnType<typeof getRuntimeApi>['updateScheduledTask']>[0]) {
  return getRuntimeApi().updateScheduledTask(input);
}

export function deleteScheduledTask(projectId: string, scheduledTaskId: string) {
  return getRuntimeApi().deleteScheduledTask(projectId, scheduledTaskId);
}

export function listScheduledTaskOccurrences(projectId: string, scheduledTaskId: string, cursor?: string | null, status?: string | null) {
  return getRuntimeApi().listScheduledTaskOccurrences(projectId, scheduledTaskId, cursor, status);
}

export function listScheduledExecutionHistory(projectId: string, scheduledTaskId: string, cursor?: string | null, anchor?: { taskId: string; runId: string } | null) {
  return getRuntimeApi().listScheduledExecutionHistory(projectId, scheduledTaskId, cursor, anchor);
}

export function deleteScheduledExecutionHistory(items: import('./types').ScheduledExecutionHistoryDeleteInputVm[]) {
  return getRuntimeApi().deleteScheduledExecutionHistory(items);
}

export function getScheduledTaskDiagnostics(projectId: string, scheduledTaskId: string) {
  return getRuntimeApi().getScheduledTaskDiagnostics(projectId, scheduledTaskId);
}

export function runScheduledTaskNow(projectId: string, scheduledTaskId: string) {
  return getRuntimeApi().runScheduledTaskNow(projectId, scheduledTaskId);
}

export function getConversationRun(projectId: string, taskId: string, runId: string, selectedSessionKey?: string | null) {
  return getRuntimeApi().getConversationRun(projectId, taskId, runId, selectedSessionKey);
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

export function listWorkspaceDirectory(projectId: string, relativePath = '') {
  return getRuntimeApi().listWorkspaceDirectory(projectId, relativePath);
}

export function openWorkspacePathInFileManager(projectId: string, relativePath = '') {
  return getRuntimeApi().openWorkspacePathInFileManager(projectId, relativePath);
}

export function listConversationDirectory(input: import('./api/client').ConversationDirectoryInput) {
  return getRuntimeApi().listConversationDirectory(input);
}

export function openConversationDirectoryPathInFileManager(input: import('./api/client').ConversationDirectoryInput) {
  return getRuntimeApi().openConversationDirectoryPathInFileManager(input);
}

export function readConversationDirectoryFile(input: import('./api/client').ConversationDirectoryInput) {
  return getRuntimeApi().readConversationDirectoryFile(input);
}

export function searchWorkspaceFiles(projectId: string, query: string, requestId: string, limit: number) {
  return getRuntimeApi().searchWorkspaceFiles(projectId, query, requestId, limit);
}

export function resolveWorkspaceFileLink(projectId: string, rawHref: string, baseCanonicalPath?: string | null) {
  return getRuntimeApi().resolveWorkspaceFileLink(projectId, rawHref, baseCanonicalPath);
}

export function readFileResource(projectId: string, canonicalPath: string, externalAccessToken?: string | null, preferSource = false) {
  return getRuntimeApi().readFileResource(projectId, canonicalPath, externalAccessToken, preferSource);
}

export function resolveMarkdownImage(input: Parameters<ReturnType<typeof getRuntimeApi>['resolveMarkdownImage']>[0]) {
  return getRuntimeApi().resolveMarkdownImage(input);
}

export function writeFileResource(input: Parameters<ReturnType<typeof getRuntimeApi>['writeFileResource']>[0]) {
  return getRuntimeApi().writeFileResource(input);
}

export function releaseWorkspaceFilePreview(token: string) {
  return getRuntimeApi().releaseWorkspaceFilePreview(token);
}

export function renewExternalFileAccess(token: string) {
  return getRuntimeApi().renewExternalFileAccess(token);
}

export function releaseExternalFileAccess(token: string) {
  return getRuntimeApi().releaseExternalFileAccess(token);
}

export function startWorkspaceFileWatch(projectId: string) {
  return getRuntimeApi().startWorkspaceFileWatch(projectId);
}

export function stopWorkspaceFileWatch(projectId: string) {
  return getRuntimeApi().stopWorkspaceFileWatch(projectId);
}

export function subscribeWorkspaceFileChanges(listener: Parameters<NonNullable<RuntimeApi['subscribeWorkspaceFileChanges']>>[0]) {
  return getRuntimeApi().subscribeWorkspaceFileChanges?.(listener) ?? Promise.resolve(() => {});
}

export function workspaceFilePreviewUrl(token: string, staticFrame = false) {
  return getRuntimeApi().workspaceFilePreviewUrl(token, staticFrame);
}

export function openExternalUrl(url: string) {
  return getRuntimeApi().openExternalUrl(url);
}

export function openFileWithSystemApp(path: string) {
  return getRuntimeApi().openFileWithSystemApp(path);
}

export function copyImageToClipboard(input: import('./api/client').ImageActionInput) {
  return getRuntimeApi().copyImageToClipboard(input);
}

export function saveImageAs(input: import('./api/client').ImageActionInput) {
  return getRuntimeApi().saveImageAs(input);
}
// pickAttachmentFiles for file picker in desktop envs
export function pickAttachmentFiles() {
  return getRuntimeApi().pickAttachmentFiles();
}

export function statAttachmentFiles(paths: string[]) {
  return getRuntimeApi().statAttachmentFiles(paths);
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

export function listMcpTools(id: string) {
  return getRuntimeApi().listMcpTools(id);
}

export function listSkills() {
  return getRuntimeApi().listSkills();
}

export function listProjectSkills(workspacePath: string) {
  return getRuntimeApi().listProjectSkills(workspacePath);
}

export function readSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null) {
  return getRuntimeApi().readSkill(name, source, workspacePath, directoryPath);
}

export function writeSkill(
  name: string,
  source: string,
  content: string,
  workspacePath?: string | null,
  oldName?: string | null,
  directoryPath?: string | null,
  syncTargets?: string[] | null,
) {
  return getRuntimeApi().writeSkill(
    name,
    source,
    content,
    workspacePath,
    oldName,
    directoryPath,
    syncTargets,
  );
}

export function deleteSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null) {
  return getRuntimeApi().deleteSkill(name, source, workspacePath, directoryPath);
}

export function updateSkillSyncTargets(
  name: string,
  source: string,
  workspacePath: string | null | undefined,
  directoryPath: string,
  syncTargets: string[],
) {
  return getRuntimeApi().updateSkillSyncTargets(name, source, workspacePath, directoryPath, syncTargets);
}

export function getSkillSyncStatus(name: string, directoryPath: string, workspacePath?: string | null) {
  return getRuntimeApi().getSkillSyncStatus(name, directoryPath, workspacePath);
}

export function checkSkillNameConflict(
  name: string,
  source: string,
  workspacePath?: string | null,
  oldName?: string | null,
  directoryPath?: string | null,
  syncTargets?: string[] | null,
) {
  return getRuntimeApi().checkSkillNameConflict(
    name,
    source,
    workspacePath,
    oldName,
    directoryPath,
    syncTargets,
  );
}
