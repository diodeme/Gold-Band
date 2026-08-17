import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AcpSessionQueryInput,
  AcpSessionVm,
  AcpUiEventVm,
  ActiveSessionStopVm,
  AgentRegistryVm,
  AppBootstrapVm,
  AppExitRequestVm,
  AutoTemplate,
  AutoTemplateStore,
  ContentVm,
  ConversationAutoConfigVm,
  ConversationCreateInput,
  ConversationRunModeVm,
  ConversationRunVm,
  ConversationSearchResultVm,
  ConversationSessionSwitchVm,
  ConversationSidebarVm,
  ConversationValidationResultVm,
  ConversationWorkspaceVm,
  InterventionNavigateEventVm,
  NotificationAttentionInput,
  ResolveAppExitInput,
  PinRef,
  CreateTaskInput,
  DesktopLanguage,
  PersonalizationPreference,
  AppearancePreference,
  LocalClaudeStatusVm,
  LogPageVm,
  LogQueryInput,
  ManagedAgentInput,
  McpServerVm,
  SkillContentVm,
  SkillListVm,
  PreferencesVm,
  ResolvedColorScheme,
  AvatarKind,
  AvatarShape,
  SaveDesktopAvatarInput,
  ImportProfilesResult,
  ProfileInput,
  ProfileListVm,
  ProfileVm,
  RoundDetailVm,
  RoundSelection,
  RunDetailVm,
  RunSummaryVm,
  TaskDetailVm,
  TaskListVm,
  UpdateBadgeStateVm,
  UpdateStatusVm,
  UpdaterSettingsVm,
  MetricsSettingsVm,
  WorkflowDsl,
  WorkflowModelBindings,
  ConversationAttemptLifecycleVm,
  ConversationQueuedPromptDraftVm,
  ConversationTaskActivityVm,
  WorkflowTemplateStore,
  WorkflowVm,
  ScheduledTaskEditVm,
  ScheduledOccurrenceVm,
  ScheduledTaskDiagnosticsVm,
  ScheduledNotificationEventVm,
  ScheduledNativeNotificationInputVm,
  ScheduledRuntimeSettingsVm,
  ScheduledRuntimeSettingsInputVm,
  RunScheduledTaskResultVm,
  UpdateScheduledTaskInput,
  FeedbackInput,
  FeedbackResult,
  FeedbackArchivePreview,
  GitCapabilityVm,
  GitCommitDetailVm,
  GitCommitReachabilityQueryVm,
  GitCommitReachabilityVm,
  GitCommitReviewQueryVm,
  GitCommitReviewVm,
  GitHistoryPageVm,
  GitHistoryQueryVm,
  GitComparisonSourceVm,
  GitFileComparisonVm,
  GitMutationRequestVm,
  GitMutationResultVm,
  GitOperationRequestVm,
  GitOperationVm,
  GitStateChangedEventVm,
  GitHubCapabilityVm,
  GitHubOperationVm,
  GitHubPullRequestCreateInputVm,
  GitHubPullRequestPreflightInputVm,
  GitHubPullRequestPreflightVm,
  GitHubPullRequestQueryVm,
  GitHubPullRequestSummaryVm,
  GitHubPullRequestDetailVm,
  GitHubIssueQueryVm,
  GitHubIssueSummaryVm,
  GitHubIssueDetailVm,
  GitSourceControlSnapshotVm,
  ExternalFileAccessGrantVm,
  FileRevisionVm,
  ResolvedWorkspaceFileLinkVm,
  WorkspaceDirectoryEntryVm,
  WorkspaceFileChangedEventVm,
  WorkspaceFileSearchVm,
  WorkspaceFileSnapshotVm,
  ResolveMarkdownImageInput,
  MarkdownImagePreviewVm,
  WriteFileResourceInput,
  TurnFileLocatorVm,
  TurnFileChangeSetVm,
  FileComparisonVm,
} from '../types';
import { browserApi } from './browser';
import { desktopApi } from './desktop';
import { isTauriRuntime } from './shared';

export interface AcpSessionUpdatedEventVm {
  branchId?: string | null;
  projectId?: string | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  session?: AcpSessionVm | null;
  event?: AcpUiEventVm | null;
  lifecycle?: ConversationAttemptLifecycleVm | null;
  activity?: ConversationTaskActivityVm | null;
}

export interface ConversationRunStateUpdatedEventVm {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  status: string;
  outcome?: string | null;
}

export interface ScheduledTaskUpdatedEventVm {
  projectId: string;
  scheduledTaskId: string;
  taskId?: string | null;
  status: string;
  task?: import('../types').ScheduledTaskVm | null;
}

export interface ScheduledOccurrenceUpdatedEventVm {
  projectId: string;
  scheduledTaskId: string;
  occurrenceId: string;
  status: string;
  errorCode?: string | null;
  taskId?: string | null;
  runId?: string | null;
}

export interface ConversationPromptSubmitVm {
  kind: 'acp-session' | 'runtime-continue-started' | 'runtime-recovery-started' | 'queued' | 'rejected' | string;
  session?: AcpSessionVm | null;
  run?: RunSummaryVm | null;
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export interface ConversationPromptQueueMutationVm {
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export interface ConversationPromptQueueRestoreVm {
  draft: ConversationQueuedPromptDraftVm;
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export interface AttachmentFileRef {
  path: string;
  name: string;
  size: number;
  previewUrl?: string | null;
  contentUrl?: string | null;
}

export interface MaterializeAttachmentFileInput {
  name: string;
  mime?: string | null;
  size: number;
  dataBase64: string;
}

export interface RuntimeApi {
  getGitCapability(projectId?: string | null): Promise<GitCapabilityVm>;
  initializeGitRepository(projectId?: string | null): Promise<GitCapabilityVm>;
  getSourceControlSnapshot(projectId: string, workspacePath?: string | null): Promise<GitSourceControlSnapshotVm>;
  getGitHistory(projectId: string, workspacePath: string | null | undefined, query: GitHistoryQueryVm): Promise<GitHistoryPageVm>;
  getGitCommitDetail(projectId: string, workspacePath: string | null | undefined, oid: string): Promise<GitCommitDetailVm>;
  getGitCommitReview(projectId: string, workspacePath: string | null | undefined, query: GitCommitReviewQueryVm): Promise<GitCommitReviewVm>;
  getGitCommitReachability(projectId: string, workspacePath: string | null | undefined, query: GitCommitReachabilityQueryVm): Promise<GitCommitReachabilityVm>;
  executeGitMutation(projectId: string, workspacePath: string | null | undefined, input: GitMutationRequestVm): Promise<GitMutationResultVm>;
  getGitComparison(projectId: string, source: GitComparisonSourceVm): Promise<GitFileComparisonVm>;
  startGitOperation(projectId: string, workspacePath: string | null | undefined, input: GitOperationRequestVm): Promise<GitOperationVm>;
  getGitOperation(operationId: string): Promise<GitOperationVm>;
  cancelGitOperation(operationId: string): Promise<GitOperationVm>;
  startGitStateMonitor(projectId: string, workspacePath: string | null | undefined): Promise<void>;
  stopGitStateMonitor(projectId: string, workspacePath: string | null | undefined): Promise<void>;
  subscribeGitOperationUpdates?(listener: (operation: GitOperationVm) => void): Promise<() => void>;
  subscribeGitStateChanges?(listener: (event: GitStateChangedEventVm) => void): Promise<() => void>;
  getGitHubCapability(projectId: string, workspacePath?: string | null): Promise<GitHubCapabilityVm>;
  startGitHubLogin(projectId: string, workspacePath: string | null | undefined, host: string): Promise<GitHubOperationVm>;
  getGitHubOperation(operationId: string): Promise<GitHubOperationVm>;
  cancelGitHubOperation(operationId: string): Promise<GitHubOperationVm>;
  subscribeGitHubOperationUpdates?(listener: (operation: GitHubOperationVm) => void): Promise<() => void>;
  preflightGitHubPullRequest(projectId: string, workspacePath: string | null | undefined, input: GitHubPullRequestPreflightInputVm): Promise<GitHubPullRequestPreflightVm>;
  startGitHubPullRequestCreate(projectId: string, workspacePath: string | null | undefined, input: GitHubPullRequestCreateInputVm): Promise<GitHubOperationVm>;
  listGitHubPullRequests(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, query: GitHubPullRequestQueryVm): Promise<GitHubPullRequestSummaryVm[]>;
  getGitHubPullRequest(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, number: number): Promise<GitHubPullRequestDetailVm>;
  listGitHubIssues(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, query: GitHubIssueQueryVm): Promise<GitHubIssueSummaryVm[]>;
  getGitHubIssue(projectId: string, workspacePath: string | null | undefined, host: string, repository: string, number: number): Promise<GitHubIssueDetailVm>;
  checkLocalClaude(): Promise<LocalClaudeStatusVm>;
  getAppBootstrap(): Promise<AppBootstrapVm>;
  completeMainWindowClose(): Promise<void>;
  resolveAppExit(input: ResolveAppExitInput): Promise<void>;
  getSystemFonts(): Promise<string[]>;
  getAgentRegistry(): Promise<AgentRegistryVm>;
  getAgentCommandCatalog(agentType: string, workspacePath: string): Promise<import('../types').AcpCommandCatalogVm | null>;
  createAgent(agentType: string, input: ManagedAgentInput): Promise<AgentRegistryVm>;
  updateAgent(agentType: string, input: ManagedAgentInput): Promise<AgentRegistryVm>;
  deleteAgent(agentType: string): Promise<AgentRegistryVm>;
  getAgentBindingUsage(agentType: string): Promise<import('../types').AgentBindingUsageVm>;
  doctorAgent(agentType: string): Promise<AgentRegistryVm>;
  getTaskList(): Promise<TaskListVm>;
  getProfiles(): Promise<ProfileListVm>;
  getProfile(id: string): Promise<ProfileVm>;
  createProfile(input: ProfileInput): Promise<ProfileVm>;
  importProfilesFromFolder(folderPath: string, dynamicTemplate: boolean): Promise<ImportProfilesResult>;
  updateProfile(id: string, input: ProfileInput): Promise<ProfileVm>;
  deleteProfile(id: string, force?: boolean): Promise<ProfileListVm>;
  chooseWorkspace(): Promise<AppBootstrapVm | null>;
  selectRecentWorkspace(workspace: string): Promise<AppBootstrapVm>;
  removeRecentWorkspace(workspace: string): Promise<AppBootstrapVm>;
  getTaskDetail(taskId: string): Promise<TaskDetailVm>;
  getWorkflow(taskId: string, projectId?: string | null): Promise<WorkflowVm>;
  createTask(input: CreateTaskInput): Promise<WorkflowVm>;
  saveTaskWorkflow(projectId: string | null | undefined, taskId: string, workflow: WorkflowDsl, modelBindings?: WorkflowModelBindings): Promise<WorkflowVm>;
  getWorkflowTemplates(): Promise<WorkflowTemplateStore>;
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl, modelBindings?: WorkflowModelBindings): Promise<WorkflowTemplateStore>;
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl, modelBindings?: WorkflowModelBindings): Promise<WorkflowTemplateStore>;
  deleteWorkflowTemplate(templateId: string): Promise<WorkflowTemplateStore>;
  getAutoTemplates(): Promise<AutoTemplateStore>;
  saveAutoTemplate(name: string, config: ConversationAutoConfigVm): Promise<AutoTemplateStore>;
  updateAutoTemplate(templateId: string, name: string, config: ConversationAutoConfigVm): Promise<AutoTemplateStore>;
  deleteAutoTemplate(templateId: string): Promise<AutoTemplateStore>;
  replaceAutoTemplates(templates: AutoTemplate[]): Promise<AutoTemplateStore>;
  getRunDetail(taskId: string, runId: string): Promise<RunDetailVm>;
  getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection): Promise<RoundDetailVm>;
  startRun(taskId: string): Promise<RunSummaryVm>;
  continueRun(projectId: string | null | undefined, taskId: string, runId: string): Promise<RunSummaryVm>;
  continueConversationRuntime(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null, input?: import('../types').ConversationPromptInput, promptId?: string | null, attachmentPaths?: string[]): Promise<ConversationPromptSubmitVm>;
  recoverConversationRuntime(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, expectedRevision: number): Promise<ConversationPromptSubmitVm>;
  pauseRun(taskId: string, runId: string, projectId?: string | null): Promise<RunSummaryVm>;
  stopActiveSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ActiveSessionStopVm>;
  submitManualCheck(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure'): Promise<RunSummaryVm>;
  retryRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  getLogPage(query: LogQueryInput): Promise<LogPageVm>;
  getAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpSessionQueryInput, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  getAcpActivityDetail(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query: import('../types').AcpActivityDetailQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<import('../types').AcpActivityDetailVm>;
  getAcpToolDetail(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query: import('../types').AcpToolDetailQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<import('../types').AcpToolDetailVm>;
  getTurnFileChangeSet(locator: TurnFileLocatorVm, changeSetId: string): Promise<TurnFileChangeSetVm>;
  getFileComparison(locator: TurnFileLocatorVm, changeSetId: string, changeId: string): Promise<FileComparisonVm>;
  renewAcpSessionLease?(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<number>;
  subscribeAcpSessionUpdates?(listener: (event: AcpSessionUpdatedEventVm) => void): Promise<() => void>;
  subscribeConversationRunStateUpdates?(listener: (event: ConversationRunStateUpdatedEventVm) => void): Promise<() => void>;
  subscribeScheduledTaskUpdates?(listener: (event: ScheduledTaskUpdatedEventVm) => void): Promise<() => void>;
  subscribeScheduledOccurrenceUpdates?(listener: (event: ScheduledOccurrenceUpdatedEventVm) => void): Promise<() => void>;
  subscribeScheduledNotifications?(listener: (event: ScheduledNotificationEventVm) => void): Promise<() => void>;
  sendScheduledNativeNotification(input: ScheduledNativeNotificationInputVm): Promise<void>;
  getScheduledRuntimeSettings(): Promise<ScheduledRuntimeSettingsVm>;
  saveScheduledRuntimeSettings(input: ScheduledRuntimeSettingsInputVm): Promise<ScheduledRuntimeSettingsVm>;
  // 干预通知：OS Toast「查看详情」点击后后端转发导航事件，前端订阅做 deep-link。
  subscribeInterventionNavigate?(listener: (event: InterventionNavigateEventVm) => void): Promise<() => void>;
  subscribeAppExitRequested?(listener: (event: AppExitRequestVm) => void): Promise<() => void>;
  takePendingInterventionNavigations(): Promise<InterventionNavigateEventVm[]>;
  submitConversationPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, input: import('../types').ConversationPromptInput, promptId?: string | null, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]): Promise<ConversationPromptSubmitVm>;
  reorderConversationQueuedPrompts(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, expectedRevision: number, orderedItemIds: string[], outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationPromptQueueMutationVm>;
  restoreConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationPromptQueueRestoreVm>;
  deleteConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationPromptQueueMutationVm>;
  useConversationQueuedPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, itemId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationPromptSubmitVm>;
  sendAcpPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]): Promise<AcpSessionVm | null>;
  setAcpSessionModel(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  setAcpSessionPermissionMode(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, permissionModeId: string | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  setAcpSessionConfigOption(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, optionId: string, optionValue: string | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  respondAcpPermission(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  respondElicitation(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, elicitationId: string, action: "accept" | "decline", content?: Record<string, unknown> | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<void>;
  cancelAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  getAcpRawFrames(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpRawFrameQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpRawFramePageVm>;
  showArtifact(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showAttachment(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showConversationAttachment(projectId: string, taskId: string, name: string): Promise<ContentVm>;
  showConversationMessageAttachment(projectId: string, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, path: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  saveDesktopPreferences(appearance: AppearancePreference, personalization: PersonalizationPreference, language: DesktopLanguage, useLocalClaude: boolean, verboseLogging: boolean): Promise<PreferencesVm>;
  saveDesktopAvatar(input: SaveDesktopAvatarInput): Promise<PreferencesVm>;
  selectRecentDesktopAvatar(kind: AvatarKind, avatarId: string): Promise<PreferencesVm>;
  saveDesktopAvatarShape(kind: AvatarKind, shape: AvatarShape | null): Promise<PreferencesVm>;
  clearDesktopAvatar(kind: AvatarKind): Promise<PreferencesVm>;
  importDesktopWallpaper(colorScheme: ResolvedColorScheme): Promise<PreferencesVm | null>;
  selectRecentDesktopWallpaper(colorScheme: ResolvedColorScheme, wallpaperId: string): Promise<PreferencesVm>;
  saveDesktopWallpaperOpacity(colorScheme: ResolvedColorScheme, opacityPercent: number): Promise<PreferencesVm>;
  restoreThemeDesktopWallpaper(colorScheme: ResolvedColorScheme): Promise<PreferencesVm>;
  saveUpdaterSettings(overrideUrl: string | null): Promise<UpdaterSettingsVm>;
  updateNotificationAttention?(input: NotificationAttentionInput): Promise<void>;
  getMetricsSettings(): Promise<MetricsSettingsVm>;
  saveMetricsSettings(enabled: boolean, metricsBaseUrl: string | null, apiKey: string | null): Promise<MetricsSettingsVm>;
  getUpdateStatus(): Promise<UpdateStatusVm>;
  markSettingsUpdateSeen(version: string): Promise<UpdateBadgeStateVm>;
  markSettingsAdvancedUpdateSeen(version: string): Promise<UpdateBadgeStateVm>;
  dismissUpdateAnnouncement(version: string): Promise<UpdateBadgeStateVm>;
  checkUpdateManual(): Promise<UpdateStatusVm>;
  downloadAndInstallUpdate(): Promise<void>;
  // ── Conversation UI ──
  saveDesktopUiMode(mode: 'conversation' | 'workbench'): Promise<void>;
  getConversationSidebar(): Promise<ConversationSidebarVm>;
  listScheduledTasks(projectId?: string | null): Promise<import('../types').ScheduledTaskVm[]>;
  setScheduledTaskEnabled(projectId: string | null | undefined, scheduledTaskId: string, enabled: boolean): Promise<import('../types').ScheduledTaskVm>;
  createScheduledTask(input: import('../types').CreateScheduledTaskInput): Promise<import('../types').ScheduledTaskVm>;
  getScheduledTask(projectId: string, scheduledTaskId: string): Promise<ScheduledTaskEditVm>;
  updateScheduledTask(input: UpdateScheduledTaskInput): Promise<ScheduledTaskEditVm>;
  deleteScheduledTask(projectId: string, scheduledTaskId: string): Promise<void>;
  listScheduledTaskOccurrences(projectId: string, scheduledTaskId: string, cursor?: string | null, status?: string | null): Promise<import('../types').ScheduledOccurrencePageVm>;
  getScheduledTaskDiagnostics(projectId: string, scheduledTaskId: string): Promise<ScheduledTaskDiagnosticsVm>;
  runScheduledTaskNow(projectId: string, scheduledTaskId: string): Promise<RunScheduledTaskResultVm>;
  getConversationWorkspaces(): Promise<ConversationWorkspaceVm[]>;
  getConversationRun(projectId: string, taskId: string, runId: string, selectedSessionKey?: string | null): Promise<ConversationRunVm>;
  switchConversationSession(projectId: string, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationSessionSwitchVm>;
  validateConversationCreate(input: ConversationCreateInput): Promise<ConversationValidationResultVm>;
  createConversationRun(input: ConversationCreateInput): Promise<ConversationRunVm>;
  rerunConversationTask(projectId: string, taskId: string): Promise<ConversationRunVm>;
  updateTaskMetadata(projectId: string, taskId: string, title: string, description?: string | null): Promise<void>;
  deleteConversationTask(projectId: string, taskId: string): Promise<ConversationSidebarVm>;
  pinConversation(projectId: string, taskId: string): Promise<ConversationSidebarVm>;
  unpinConversation(projectId: string, taskId: string): Promise<ConversationSidebarVm>;
  reorderPinnedConversations(pins: PinRef[]): Promise<ConversationSidebarVm>;
  searchConversationTasks(query: string, limit?: number): Promise<ConversationSearchResultVm[]>;
  getConversationRunMode(projectId: string): Promise<ConversationRunModeVm | null>;
  saveConversationRunMode(projectId: string, settings: ConversationRunModeVm): Promise<void>;
  chooseConversationWorkspace(): Promise<ConversationWorkspaceVm>;
  addConversationWorkspace(): Promise<ConversationSidebarVm>;
  removeConversationWorkspace(projectId: string): Promise<ConversationSidebarVm>;
  syncConversationWorkspace(workspacePath: string): Promise<ConversationSidebarVm>;
  saveConversationPreference(key: string, value: unknown): Promise<void>;
  saveLastConversationWorkspace(projectId: string): Promise<void>;
  listWorkspaceDirectory(projectId: string, relativePath: string): Promise<WorkspaceDirectoryEntryVm[]>;
  openWorkspacePathInFileManager(projectId: string, relativePath?: string): Promise<void>;
  listConversationDirectory(input: ConversationDirectoryInput): Promise<WorkspaceDirectoryEntryVm[]>;
  openConversationDirectoryPathInFileManager(input: ConversationDirectoryInput): Promise<void>;
  readConversationDirectoryFile(input: ConversationDirectoryInput): Promise<WorkspaceFileSnapshotVm>;
  searchWorkspaceFiles(projectId: string, query: string, requestId: string, limit: number): Promise<WorkspaceFileSearchVm>;
  resolveWorkspaceFileLink(projectId: string, rawHref: string, baseCanonicalPath?: string | null): Promise<ResolvedWorkspaceFileLinkVm>;
  readFileResource(projectId: string, canonicalPath: string, externalAccessToken?: string | null, preferSource?: boolean): Promise<WorkspaceFileSnapshotVm>;
  resolveMarkdownImage(input: ResolveMarkdownImageInput): Promise<MarkdownImagePreviewVm>;
  writeFileResource(input: WriteFileResourceInput): Promise<FileRevisionVm>;
  releaseWorkspaceFilePreview(token: string): Promise<void>;
  renewExternalFileAccess(token: string): Promise<ExternalFileAccessGrantVm>;
  releaseExternalFileAccess(token: string): Promise<void>;
  startWorkspaceFileWatch(projectId: string): Promise<void>;
  stopWorkspaceFileWatch(projectId: string): Promise<void>;
  subscribeWorkspaceFileChanges?(listener: (event: WorkspaceFileChangedEventVm) => void): Promise<() => void>;
  workspaceFilePreviewUrl(token: string, staticFrame?: boolean): string;
  openExternalUrl(url: string): Promise<void>;
  openFileWithSystemApp(path: string): Promise<void>;
  pickAttachmentFiles(): Promise<AttachmentFileRef[]>;
  statAttachmentFiles(paths: string[]): Promise<AttachmentFileRef[]>;
  materializeConversationAttachments(files: MaterializeAttachmentFileInput[]): Promise<AttachmentFileRef[]>;
  getSupportedAttachmentExtensions(): Promise<string[]>;
  openInFileManager(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId?: string | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<void>;
  // MCP & SKILL management
  listMcpServers(): Promise<McpServerVm[]>;
  addMcpServer(jsonContent: string): Promise<McpServerVm[]>;
  updateMcpServer(id: string, jsonContent: string): Promise<McpServerVm[]>;
  deleteMcpServer(id: string): Promise<McpServerVm[]>;
  toggleMcpServer(id: string, enabled: boolean): Promise<McpServerVm[]>;
  checkMcpServerHealth(id: string): Promise<import('../types').McpServerHealthResult>;
  listMcpTools(id: string): Promise<import('../types').ToolInfo[]>;
  listSkills(): Promise<SkillListVm>;
  listProjectSkills(workspacePath: string): Promise<import('../types').SkillMetaVm[]>;
  readSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null): Promise<SkillContentVm>;
  writeSkill(
    name: string,
    source: string,
    content: string,
    workspacePath?: string | null,
    oldName?: string | null,
    directoryPath?: string | null,
    syncTargets?: string[] | null,
  ): Promise<SkillListVm>;
  deleteSkill(name: string, source: string, workspacePath?: string | null, directoryPath?: string | null): Promise<SkillListVm>;
  updateSkillSyncTargets(
    name: string,
    source: string,
    workspacePath: string | null | undefined,
    directoryPath: string,
    syncTargets: string[],
  ): Promise<SkillListVm>;
  getSkillSyncStatus(name: string, directoryPath: string, workspacePath?: string | null): Promise<import('../types').SyncStatusEntryVm[]>;
  checkSkillNameConflict(
    name: string,
    source: string,
    workspacePath?: string | null,
    oldName?: string | null,
    directoryPath?: string | null,
    syncTargets?: string[] | null,
  ): Promise<string[]>;
  submitFeedback(input: FeedbackInput): Promise<FeedbackResult>;
  previewFeedbackSessionArchive(projectId: string | null, taskId: string | null): Promise<FeedbackArchivePreview | null>;
}

export interface ConversationDirectoryInput {
  projectId?: string | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  relativePath?: string;
}

export function getRuntimeApi(): RuntimeApi {
  return isTauriRuntime() ? desktopApi : browserApi;
}
