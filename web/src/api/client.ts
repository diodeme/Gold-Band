import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AcpSessionQueryInput,
  AcpSessionVm,
  AcpUiEventVm,
  ActiveSessionStopVm,
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
  InterventionNavigateEventVm,
  PinRef,
  CreateTaskInput,
  DesktopFontPreference,
  DesktopLanguage,
  DesktopThemePreference,
  LocalClaudeStatusVm,
  LogPageVm,
  LogQueryInput,
  ManagedAgentInput,
  McpServerVm,
  SkillContentVm,
  SkillListVm,
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
}

export interface AttachmentFileRef {
  path: string;
  name: string;
  size: number;
}

export interface MaterializeAttachmentFileInput {
  name: string;
  mime?: string | null;
  size: number;
  dataBase64: string;
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
  saveTaskWorkflow(projectId: string | null | undefined, taskId: string, workflow: WorkflowDsl): Promise<WorkflowVm>;
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
  continueRun(projectId: string | null | undefined, taskId: string, runId: string, promptId?: string | null, prompt?: string | null): Promise<RunSummaryVm>;
  pauseRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  stopActiveSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ActiveSessionStopVm>;
  submitManualCheck(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outcome: 'success' | 'failure'): Promise<RunSummaryVm>;
  retryRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  killRun(taskId: string, runId: string): Promise<RunSummaryVm>;
  getLogPage(query: LogQueryInput): Promise<LogPageVm>;
  getAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpSessionQueryInput, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  subscribeAcpSessionUpdates?(listener: (event: AcpSessionUpdatedEventVm) => void): Promise<() => void>;
  // 干预通知：OS Toast「查看详情」点击后后端转发导航事件，前端订阅做 deep-link。
  subscribeInterventionNavigate?(listener: (event: InterventionNavigateEventVm) => void): Promise<() => void>;
  sendAcpPrompt(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, prompt: string, promptId?: string | null, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null, attachmentPaths?: string[]): Promise<AcpSessionVm | null>;
  setAcpSessionModel(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, modelId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  setAcpSessionPermissionMode(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, permissionModeId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  respondAcpPermission(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, requestId: string, optionId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  cancelAcpSession(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, fallback?: AcpSessionVm | null, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpSessionVm | null>;
  getAcpRawFrames(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, query?: AcpRawFrameQueryInput, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<AcpRawFramePageVm>;
  showArtifact(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showAttachment(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, name: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  showConversationAttachment(projectId: string, taskId: string, name: string): Promise<ContentVm>;
  showWorkerRef(taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null): Promise<ContentVm>;
  saveDesktopPreferences(theme: DesktopThemePreference, language: DesktopLanguage, font: DesktopFontPreference, useLocalClaude: boolean, verboseLogging: boolean): Promise<PreferencesVm>;
  saveUpdaterSettings(overrideUrl: string | null): Promise<UpdaterSettingsVm>;
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
  pickAttachmentFiles(): Promise<AttachmentFileRef[]>;
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
  listSkills(): Promise<SkillListVm>;
  listProjectSkills(workspacePath: string): Promise<import('../types').SkillMetaVm[]>;
  readSkill(name: string, source: string, workspacePath?: string | null): Promise<SkillContentVm>;
  writeSkill(name: string, source: string, content: string, workspacePath?: string | null, oldName?: string | null): Promise<SkillListVm>;
  deleteSkill(name: string, source: string, workspacePath?: string | null): Promise<SkillListVm>;
}

export function getRuntimeApi(): RuntimeApi {
  return isTauriRuntime() ? desktopApi : browserApi;
}
