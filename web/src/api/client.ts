import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AcpSessionQueryInput,
  AcpSessionVm,
  AcpUiEventVm,
  AgentRegistryVm,
  AppBootstrapVm,
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
  PinRef,
  CreateTaskInput,
  DesktopFontPreference,
  DesktopLanguage,
  DesktopThemePreference,
  LocalClaudeStatusVm,
  LogPageVm,
  LogQueryInput,
  ManagedAgentInput,
  PreferencesVm,
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
  WorkflowTemplateStore,
  WorkflowVm,
} from '../types';
import { browserApi } from './browser';
import { desktopApi } from './desktop';
import { isTauriRuntime } from './shared';

export interface AcpSessionUpdatedEventVm {
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  session?: AcpSessionVm | null;
  event?: AcpUiEventVm | null;
}

export interface RuntimeApi {
  checkLocalClaude(): Promise<LocalClaudeStatusVm>;
  getAppBootstrap(): Promise<AppBootstrapVm>;
  getSystemFonts(): Promise<string[]>;
  getAgentRegistry(): Promise<AgentRegistryVm>;
  createAgent(agentType: string, input: ManagedAgentInput): Promise<AgentRegistryVm>;
  updateAgent(agentType: string, input: ManagedAgentInput): Promise<AgentRegistryVm>;
  deleteAgent(agentType: string): Promise<AgentRegistryVm>;
  doctorAgent(agentType: string): Promise<AgentRegistryVm>;
  getTaskList(): Promise<TaskListVm>;
  getProfiles(): Promise<ProfileListVm>;
  getProfile(id: string): Promise<ProfileVm>;
  createProfile(input: ProfileInput): Promise<ProfileVm>;
  updateProfile(id: string, input: ProfileInput): Promise<ProfileVm>;
  deleteProfile(id: string, force?: boolean): Promise<ProfileListVm>;
  chooseWorkspace(): Promise<AppBootstrapVm | null>;
  selectRecentWorkspace(workspace: string): Promise<AppBootstrapVm>;
  getTaskDetail(taskId: string): Promise<TaskDetailVm>;
  getWorkflow(taskId: string): Promise<WorkflowVm>;
  createTask(input: CreateTaskInput): Promise<WorkflowVm>;
  saveTaskWorkflow(taskId: string, workflow: WorkflowDsl): Promise<WorkflowVm>;
  getWorkflowTemplates(): Promise<WorkflowTemplateStore>;
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl): Promise<WorkflowTemplateStore>;
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl): Promise<WorkflowTemplateStore>;
  deleteWorkflowTemplate(templateId: string): Promise<WorkflowTemplateStore>;
  getAutoTemplates(): Promise<AutoTemplateStore>;
  saveAutoTemplate(name: string, config: ConversationAutoConfigVm): Promise<AutoTemplateStore>;
  updateAutoTemplate(templateId: string, name: string, config: ConversationAutoConfigVm): Promise<AutoTemplateStore>;
  deleteAutoTemplate(templateId: string): Promise<AutoTemplateStore>;
  replaceAutoTemplates(templates: AutoTemplate[]): Promise<AutoTemplateStore>;
  getRunDetail(taskId: string, runId: string): Promise<RunDetailVm>;
  getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection): Promise<RoundDetailVm>;
  startRun(taskId: string): Promise<RunSummaryVm>;
  continueRun(taskId: string, runId: string, promptId?: string | null): Promise<RunSummaryVm>;
  submitManualCheck(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure'): Promise<RunSummaryVm>;
  retryRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  killRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  getLogPage(query: LogQueryInput): Promise<LogPageVm>;
  getAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpSessionQueryInput, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  subscribeAcpSessionUpdates?(listener: (event: AcpSessionUpdatedEventVm) => void): Promise<() => void>;
  sendAcpPrompt(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]): Promise<AcpSessionVm | null>;
  setAcpSessionModel(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  setAcpSessionPermissionMode(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, permissionModeId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  respondAcpPermission(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  cancelAcpSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  getAcpRawFrames(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpRawFrameQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpRawFramePageVm>;
  showArtifact(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showAttachment(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showConversationAttachment(taskId: string, name: string): Promise<ContentVm>;
  showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean): Promise<PreferencesVm>;
  saveUpdaterSettings(overrideUrl: string | null): Promise<UpdaterSettingsVm>;
  getMetricsSettings(): Promise<MetricsSettingsVm>;
  saveMetricsSettings(enabled: boolean, heartbeatEndpoint: string | null, nodeMetricsEndpoint: string | null, apiKey: string | null): Promise<MetricsSettingsVm>;
  getUpdateStatus(): Promise<UpdateStatusVm>;
  markSettingsUpdateSeen(version: string): Promise<UpdateBadgeStateVm>;
  markSettingsAdvancedUpdateSeen(version: string): Promise<UpdateBadgeStateVm>;
  dismissUpdateAnnouncement(version: string): Promise<UpdateBadgeStateVm>;
  checkUpdateManual(): Promise<UpdateStatusVm>;
  downloadAndInstallUpdate(): Promise<void>;
  // ── Conversation UI ──
  saveDesktopUiMode(mode: 'conversation' | 'workbench'): Promise<void>;
  getConversationSidebar(): Promise<ConversationSidebarVm>;
  getConversationRun(projectId: string, taskId: string, runId: string, selectedSessionKey?: string | null): Promise<ConversationRunVm>;
  switchConversationSession(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ConversationSessionSwitchVm>;
  validateConversationCreate(input: ConversationCreateInput): Promise<ConversationValidationResultVm>;
  createConversationRun(input: ConversationCreateInput): Promise<ConversationRunVm>;
  rerunConversationTask(projectId: string, taskId: string): Promise<ConversationRunVm>;
  updateTaskMetadata(projectId: string, taskId: string, title: string, description?: string | null): Promise<void>;
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
  pickAttachmentFiles(): Promise<Array<{ path: string; name: string; size: number }>>;
  getSupportedAttachmentExtensions(): Promise<string[]>;
  openInFileManager(taskId: string, runId: string, roundId: string, nodeId: string, attemptId?: string | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<void>;
}

export function getRuntimeApi(): RuntimeApi {
  return isTauriRuntime() ? desktopApi : browserApi;
}
