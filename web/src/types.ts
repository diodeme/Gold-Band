export type DesktopThemePreference = 'system' | 'light' | 'light-warm' | 'dark' | 'black';
export type ConcreteDesktopTheme = Exclude<DesktopThemePreference, 'system'>;
export type DesktopThemeMode = 'light' | 'dark';
export type DesktopFontPreference = string;
export type DesktopLanguage = 'zh-cn' | 'en';
export type UpdateCheckStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'not-available' | 'error';

export interface StartupCheckResult {
  critical: boolean;
  error?: string | null;
}

export interface PreferencesVm {
  theme: DesktopThemePreference;
  language: DesktopLanguage;
  font: DesktopFontPreference;
  useLocalClaude: boolean;
}

export interface LocalClaudeStatusVm {
  found: boolean;
  path?: string | null;
}

export interface UpdaterSettingsVm {
  channel: string;
  builtInUrl: string;
  overrideUrl?: string | null;
  effectiveUrl: string;
  pollIntervalMinutes: number;
}

export interface UpdateInfoVm {
  version: string;
  currentVersion: string;
  notes?: string | null;
  pubDate?: string | null;
}

export interface UpdateStatusVm {
  status: UpdateCheckStatus;
  checkedAt?: string | null;
  update?: UpdateInfoVm | null;
  error?: AppErrorVm | null;
  background: boolean;
}

export interface UpdateBadgeStateVm {
  settingsEntrySeenVersion?: string | null;
  settingsAdvancedSeenVersion?: string | null;
  announcementClosedVersion?: string | null;
}

export interface AppBootstrapVm {
  repoRoot: string;
  recentWorkspaces: string[];
  preferences: PreferencesVm;
  updaterSettings: UpdaterSettingsVm;
  updateStatus: UpdateStatusVm;
  updateBadges: UpdateBadgeStateVm;
  persistedAvailableUpdate?: UpdateInfoVm | null;
  clientVersion: string;
  appInfo: AppInfoVm;
  appConfig: AppConfigVm;
  needsWorkspace: boolean;
}

export interface AppConfigVm {
  acpSessionTitleRefreshEnabled: boolean;
  acpChatEventPageSize: number;
}

export interface AppInfoVm {
  channel: string;
  appName: string;
  appKey: string;
  configDirName: string;
}

export interface AgentRegistryVm {
  agents: ManagedAgentVm[];
  supportedTypes: SupportedAgentTypeVm[];
}

export interface ManagedAgentVm {
  agentType: string;
  displayName: string;
  command: string;
  args: string[];
  env: AgentEnvEntryVm[];
  iconKey: string;
  supported: boolean;
  diagnostic?: ManagedAgentDiagnosticVm | null;
  supportedModes?: AcpModeVm[] | null;
  supportedModels?: AcpModeVm[] | null;
}

export interface AcpModeVm {
  id: string;
  name: string;
  description?: string | null;
}

export interface AcpUsageVm {
  used?: number | null;
  size?: number | null;
  costAmountUsd?: number | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cachedReadTokens?: number | null;
  cachedWriteTokens?: number | null;
  totalTokens?: number | null;
}

export interface AgentEnvEntryVm {
  key: string;
  value: string;
}

export interface ManagedAgentDiagnosticVm {
  status: string;
  available: boolean;
  reason?: string | null;
  checkedAt: string;
}

export interface SupportedAgentTypeVm {
  agentType: string;
  label: string;
  iconKey: string;
  supported: boolean;
  configured: boolean;
  defaultDisplayName: string;
  defaultCommand: string;
  defaultArgs: string[];
  defaultEnv: AgentEnvEntryVm[];
}

export interface ManagedAgentInput {
  displayName: string;
  command: string;
  args: string[];
  env: Record<string, string>;
}

export interface SummaryCardVm {
  key: string;
  label: string;
  value: number;
  tone: string;
}

export interface TaskListVm {
  cards: SummaryCardVm[];
  tasks: TaskRowVm[];
}

export interface TaskRowVm {
  id: string;
  title: string;
  description?: string | null;
  requirement: string;
  requirementPreview: string;
  displayStatus: string;
  workflowExists: boolean;
  workflowValid: boolean;
  workflowError?: WorkflowErrorVm | null;
  latestRun?: RunSummaryVm | null;
  resumableRunId?: string | null;
  artifactCount: number;
  attachmentCount: number;
}

export interface AppErrorVm {
  code: string;
  params: Record<string, unknown>;
}

export type WorkflowErrorVm = AppErrorVm;

export interface TaskDetailVm {
  task: TaskRowVm;
  requirement: string;
  runs: RunSummaryVm[];
}

export interface WorkflowVm {
  task: TaskRowVm;
  graph: GraphVm;
  runs: RunGroupVm[];
  control?: WorkflowControlVm | null;
  workflowJson?: string | null;
}

export interface WorkflowDsl {
  version: string;
  id: string;
  entry: string;
  control: WorkflowControlDsl;
  nodes: WorkflowNodeDsl[];
  edges: WorkflowEdgeDsl[];
}

export interface WorkflowControlDsl {
  max_attempts?: number | null;
  max_rounds?: number | null;
}

export type WorkflowNodeDsl = WorkflowWorkerNodeDsl | WorkflowAiDynamicNodeDsl;

export interface WorkflowWorkerNodeDsl {
  type: 'worker';
  id: string;
  provider?: string | null;
  model?: string | null;
  profile?: string | null;
  goal?: string | null;
  output?: WorkflowOutputContractDsl | null;
  success_condition?: WorkflowJsonConditionDsl | null;
  permission_mode?: string | null;
  manual_check?: boolean | null;
}

export type WorkflowAiDynamicAgentStrategyDsl = WorkflowAiDynamicFixedAgentStrategyDsl | WorkflowAiDynamicDynamicAgentStrategyDsl;

export interface DynamicAgentRefDsl {
  provider: string;
  model: string;
}

export interface WorkflowAiDynamicFixedAgentStrategyDsl {
  mode: 'fixed';
  provider: string;
  model?: string;
}

export interface WorkflowAiDynamicDynamicAgentStrategyDsl {
  mode: 'dynamic';
  bootstrapProvider: string;
  routingPrompt: string;
  availableAgents: DynamicAgentRefDsl[];
}

export interface WorkflowAiDynamicNodeDsl {
  type: 'ai-dynamic';
  id: string;
  agentStrategy: WorkflowAiDynamicAgentStrategyDsl;
  permission_mode?: string | null;
  allowedProfiles?: string[];
  globalGoal?: string | null;
  control: DynamicControlDsl;
  allowedWorkflows: AllowedWorkflowRefDsl[];
}

export interface DynamicControlDsl {
  maxDynamicNodes: number;
  maxFanout: number;
  maxDepth: number;
  maxParallel: number;
  maxGroupDepth: number;
  maxWorkflowInvocations: number;
  allowNestedDynamic: boolean;
}

export interface AllowedWorkflowRefDsl {
  workflowId: string;
}

export interface WorkflowOutputContractDsl {
  kind: 'json' | string;
  artifact: string;
  schema?: unknown | null;
}

export type WorkflowJsonConditionDsl =
  | { expression: string; path?: never; equals?: never }
  | { path: string; equals: unknown; expression?: never };

export interface WorkflowEdgeDsl {
  from: string;
  to: string;
  on: 'success' | 'failure' | string;
  session?: 'new' | 'continue' | null;
}

export interface CreateTaskInput {
  title?: string | null;
  description?: string | null;
  requirementFileName?: string | null;
  requirementContent: string;
  workflow: WorkflowDsl;
  workflowTemplateId?: string | null;
}

export interface WorkflowTemplateStore {
  version: string;
  lastUsedTemplateId?: string | null;
  lastCreatedWorkflow?: WorkflowDsl | null;
  templates: WorkflowTemplate[];
}

export interface WorkflowTemplate {
  id: string;
  name: string;
  workflow: WorkflowDsl;
  createdAt: string;
  updatedAt: string;
}

export type ProfileScope = 'built-in' | 'user' | 'project';

export interface ProfileVm {
  id: string;
  name: string;
  summary: string;
  content: string;
  scope: ProfileScope;
  isBuiltIn: boolean;
  createdAt: string;
  updatedAt: string;
  path: string;
}

export interface ProfileListVm {
  profiles: ProfileVm[];
}

export interface ProfileInput {
  scope: ProfileScope;
  name: string;
  summary: string;
  content: string;
}

export interface SaveWorkflowInput {
  workflow: WorkflowDsl;
}

export interface WorkflowControlVm {
  maxAttempts?: number | null;
  maxRounds?: number | null;
}

export interface RunDetailVm {
  run: RunSummaryVm;
  rounds: RoundSummaryVm[];
  events?: string | null;
  progress?: unknown;
}

export interface RoundDetailVm {
  run: RunSummaryVm;
  round: RoundSummaryVm;
  graph: GraphVm;
  control?: WorkflowControlVm | null;
  controlFailure?: ControlFailureVm | null;
  requirement: string;
  selectedNodeDetail?: NodeDetailVm | null;
}

export interface ControlFailureVm {
  reasonKind: string;
  title: string;
  message: string;
  fromNodeId?: string | null;
  toNodeId?: string | null;
  target?: string | null;
  edgeOutcome?: string | null;
  proposedCount?: number | null;
  limit?: number | null;
  timestamp?: string | null;
  roundId?: string | null;
  nodeId?: string | null;
  attemptId?: string | null;
}

export interface RunGroupVm {
  run: RunSummaryVm;
  rounds: RoundSummaryVm[];
}

export interface RunSummaryVm {
  id: string;
  taskId: string;
  status: string;
  outcome?: string | null;
  startedAt: string;
  updatedAt: string;
  currentRound?: string | null;
  currentNode?: string | null;
  currentAttempt?: string | null;
  resumable: boolean;
  pauseReason?: string | null;
}

export interface RoundSummaryVm {
  id: string;
  runId: string;
  index: number;
  status: string;
  outcome?: string | null;
  trigger: string;
  startedAt: string;
  currentNode?: string | null;
  artifactCount: number;
  attachmentCount: number;
}

export interface GraphVm {
  nodes: GraphNodeVm[];
  edges: GraphEdgeVm[];
}

export interface GraphNodeVm {
  id: string;
  nodeId?: string | null;
  sequence?: number | null;
  label: string;
  nodeType: string;
  status?: string | null;
  outcome?: string | null;
  attemptId?: string | null;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  attemptCount?: number;
  attempts?: GraphAttemptVm[];
  artifactCount: number;
  attachmentCount: number;
  current: boolean;
  iconKey?: string | null;
  sessionMode?: string | null;
  continueFromNodeId?: string | null;
  dynamicSummary?: DynamicSummaryVm | null;
  dynamicGroupId?: string | null;
}

export interface GraphAttemptVm {
  attemptId: string;
  sequence?: number | null;
  status: string;
  outcome?: string | null;
  sessionMode?: string | null;
  acpSessionId?: string | null;
  current: boolean;
}

export interface GraphEdgeVm {
  from: string;
  to: string;
  label: string;
  traversalCount?: number;
  lastOutcome?: string | null;
  blockedReason?: ControlFailureVm | null;
}

export interface DynamicSummaryVm {
  status: string;
  outcome?: string | null;
  internalNodeCount: number;
  groupCount: number;
  proposalCount: number;
  currentNodeIds: string[];
}

export interface DynamicGroupVm {
  id: string;
  status: string;
  depth: number;
  parentGroupId?: string | null;
  rootNodeIds: string[];
  terminalNodeIds: string[];
  mergeNodeId?: string | null;
  acceptanceNodeId?: string | null;
}

export interface DynamicProposalValidationErrorVm {
  code: string;
  message: string;
  params: Record<string, unknown>;
}

export interface DynamicProposalVm {
  id: string;
  sourceNodeId: string;
  validationStatus: string;
  validationErrors: DynamicProposalValidationErrorVm[];
  artifactPath: string;
  createdAt: string;
}

export interface DynamicDetailVm {
  summary: DynamicSummaryVm;
  graph: GraphVm;
  groups: DynamicGroupVm[];
  proposals: DynamicProposalVm[];
}

export interface NodeDetailVm {
  id: string;
  nodeId: string;
  sequence?: number | null;
  label: string;
  nodeType: string;
  provider?: string | null;
  providerDisplayName?: string | null;
  status: string;
  outcome?: string | null;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  current: boolean;
  startedAt: string;
  finishedAt?: string | null;
  artifactCount: number;
  attachmentCount: number;
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
  hasProgressEvents: boolean;
  hasRawStream: boolean;
  hasWorkerRef: boolean;
  manualCheckEnabled: boolean;
  manualCheckPending: boolean;
  sessionMode?: string | null;
  continueFromNodeId?: string | null;
  acpSession?: AcpSessionVm | null;
  acpConversations?: AcpConversationVm[];
  selectedConversationKey?: string | null;
  dynamic?: DynamicDetailVm | null;
  dynamicGroupId?: string | null;
}

export interface AcpConversationVm {
  key: string;
  label: string;
  sessionId?: string | null;
  sessionMode: string;
  activeAttemptId: string;
  attempts: AcpAttemptSessionVm[];
}

export interface AcpAttemptSessionVm {
  nodeId: string;
  attemptId: string;
  sequence?: number | null;
  status: string;
  outcome?: string | null;
  current: boolean;
  sessionMode?: string | null;
  acpSessionId?: string | null;
  acpSession?: AcpSessionVm | null;
}

export interface AcpSessionVm {
  sessionId?: string | null;
  title?: string | null;
  provider: string;
  adapterId?: string | null;
  adapterDisplayName?: string | null;
  cwd?: string | null;
  status: string;
  sessionStartedAt?: string | null;
  sessionUpdatedAt?: string | null;
  sessionElapsedSeconds?: number | null;
  restored: boolean;
  stopReason?: string | null;
  systemPromptAppend?: string | null;
  config?: AcpSessionConfigVm | null;
  events: AcpUiEventVm[];
  eventPage: AcpEventPageVm;
  pendingPermissions: AcpPermissionRequestVm[];
  availableCommands?: unknown[] | null;
  usage?: AcpUsageVm | null;
  diagnostics: AcpDiagnosticsVm;
}

export interface AcpSessionQueryInput {
  beforeSeq?: number;
  afterSeq?: number;
  beforeCursor?: string;
  afterCursor?: string;
  eventLimit?: number;
  pageSize?: number;
}

export interface AcpEventPageVm {
  loadedCount: number;
  total: number;
  oldestSeq?: number | null;
  newestSeq?: number | null;
  hasOlder: boolean;
  hasNewer: boolean;
  oldestCursor?: string | null;
  newestCursor?: string | null;
}

export interface AcpSessionConfigVm {
  currentModelId?: string | null;
  currentModelName?: string | null;
  currentModeId?: string | null;
  currentModeName?: string | null;
  models?: unknown | null;
  modes?: unknown | null;
  configOptions?: unknown | null;
}

export interface AcpUiEventVm {
  id: string;
  seq: number;
  timestamp: string;
  kind: string;
  sessionId?: string | null;
  content?: string | null;
  title?: string | null;
  toolCallId?: string | null;
  status?: string | null;
  startedSeq?: number | null;
  endedSeq?: number | null;
  startedAt?: string | null;
  endedAt?: string | null;
  raw?: unknown;
}

export interface AcpPermissionRequestVm {
  requestId: string;
  title: string;
  toolCallId?: string | null;
  options: AcpPermissionOptionVm[];
  raw: unknown;
}

export interface AcpPermissionOptionVm {
  optionId: string;
  name: string;
  kind: string;
}

export interface AcpDiagnosticsVm {
  rawFrameCount: number;
  eventCount: number;
  errorCount: number;
  lastError?: string | null;
  lastErrorTimestamp?: string | null;
}

export interface AcpRawFrameQueryInput {
  page?: number;
  pageSize?: number;
  search?: string;
  kind?: string;
  direction?: string;
}

export interface AcpRawFrameVm {
  id: string;
  lineNumber: number;
  timestamp?: string | null;
  direction?: string | null;
  kind: string;
  content: string;
  contentTruncated: boolean;
}

export interface AcpRawFramePageVm {
  items: AcpRawFrameVm[];
  page: number;
  pageSize: number;
  total: number;
  hasPrevious: boolean;
  hasNext: boolean;
  order: string;
  search?: string | null;
  kind?: string | null;
  direction?: string | null;
}

export interface AssetItemVm {
  kind: 'artifact' | 'attachment' | string;
  name: string;
  title: string;
  tone: string;
  preview: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
}

export interface AttachmentMetaVm {
  name: string;
  path: string;
  type: string;
  size: number;
}

export interface LogEntryVm {
  id: string;
  timestamp: string;
  entryType: string;
  level?: string | null;
  nodeId?: string | null;
  attemptId?: string | null;
  stage?: string | null;
  summary: string;
  source: string;
  raw: unknown;
}

export interface LogPageVm {
  items: LogEntryVm[];
  page: number;
  pageSize: number;
  total: number;
  hasPrevious: boolean;
  hasNext: boolean;
  tier: string;
  hotLimit: number;
  archiveRetentionDays: number;
}

export interface LogScopeInput {
  taskId: string;
  runId: string;
  roundId?: string | null;
  nodeId?: string | null;
  attemptId?: string | null;
}

export interface LogQueryInput {
  scope: LogScopeInput;
  source?: 'system' | 'run-events' | 'progress-events' | 'raw-stream' | string;
  page?: number;
  pageSize?: number;
  hotLimit?: number;
}

export interface StreamItemVm {
  id: string;
  title: string;
  kind: string;
  tone: string;
  content: string;
  nodeId?: string | null;
  attemptId?: string | null;
  name?: string | null;
}

export interface ContentVm {
  title: string;
  kind: string;
  content: string;
  metadata: unknown;
}

export type PrimaryModule = 'task-orchestration' | 'agent-management' | 'knowledge-base' | 'settings';

export type TaskPage =
  | { kind: 'task-list' }
  | { kind: 'workflow'; taskId: string }
  | { kind: 'round-detail'; taskId: string; runId: string; roundId: string };

type RoundSelectionContext = { contextNodeId?: string };

export type RoundSelection = RoundSelectionContext & (
  | { kind: 'round' }
  | { kind: 'requirement' }
  | { kind: 'node'; nodeId: string; attemptId?: string; outerNodeId?: string; outerAttemptId?: string }
  | { kind: 'artifact'; nodeId: string; attemptId: string; name: string }
  | { kind: 'attachment'; nodeId: string; attemptId: string; name: string }
  | { kind: 'worker-ref'; nodeId: string; attemptId: string }
  | { kind: 'event'; id: string; nodeId?: string; attemptId?: string }
  | { kind: 'log'; id: string; nodeId?: string; attemptId?: string }
);

// ── Conversation UI types ──

export type DesktopUiMode = 'conversation' | 'workbench';

export type ConversationPage =
  | { kind: 'conversation-home' }
  | { kind: 'conversation-run'; projectId: string; taskId: string; runId: string }
  | { kind: 'run-mode-management' }
  | { kind: 'agents' }
  | { kind: 'contexts' }
  | { kind: 'settings' };

export interface ConversationWorkspaceVm {
  projectId: string;
  workspacePath: string;
  name: string;
}

export interface ConversationTaskRowVm {
  projectId: string;
  taskId: string;
  title: string;
  autoTitle: boolean;
  runMode: 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  latestRun?: ConversationRunSummaryVm | null;
  runs: ConversationRunSummaryVm[];
  pinned: boolean;
  pinnedOrder?: number | null;
}

export interface ConversationRunSummaryVm {
  runId: string;
  status: string;
  outcome?: string | null;
  startedAt: string;
  updatedAt: string;
  currentRound?: string | null;
  currentNode?: string | null;
  resumable: boolean;
}

export interface ConversationSidebarVm {
  workspaces: ConversationWorkspaceVm[];
  pinnedTasks: ConversationTaskRowVm[];
  tasksByWorkspace: Record<string, ConversationTaskRowVm[]>;
  lastActiveWorkspaceId?: string | null;
  preferences?: Record<string, unknown> | null;
}

export interface PinRef {
  projectId: string;
  taskId: string;
}

export interface ConversationSessionLeafVm {
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  pathLabel: string;
  status: string;
  outcome?: string | null;
  current: boolean;
  startedAt?: string | null;
  finishedAt?: string | null;
  sessionId?: string | null;
  artifactCount: number;
  attachmentCount: number;
}

export interface ConversationSessionTreeVm {
  rounds: ConversationRoundNodeVm[];
  selectedSessionKey?: string | null;
}

export interface ConversationRoundNodeVm {
  roundId: string;
  index: number;
  label: string;
  status: string;
  nodes: ConversationTreeNodeVm[];
}

export interface ConversationTreeNodeVm {
  nodeId: string;
  label: string;
  nodeType: string;
  status: string;
  attempts: ConversationSessionLeafVm[];
  outerNodes?: ConversationTreeNodeVm[];
}

export interface ConversationRunVm {
  projectId: string;
  taskId: string;
  runId: string;
  title: string;
  autoTitle: boolean;
  runMode: 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  runStatus: string;
  runOutcome?: string | null;
  sessionTree: ConversationSessionTreeVm;
  selectedSession?: AcpSessionVm | null;
  activeSessions: ConversationActiveSessionVm[];
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
  inputAttachments: AssetItemVm[];
  workflowStatus: string;
  workflowValid: boolean;
  workflowError?: WorkflowErrorVm | null;
  workflowJson?: string | null;
  workflowGraph: GraphVm;
  resumable: boolean;
  pauseReason?: string | null;
}

export interface ConversationSessionSwitchVm {
  selectedSession?: AcpSessionVm | null;
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
}

export interface ConversationActiveSessionVm {
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  pathLabel: string;
  status: string;
  sessionId?: string | null;
  startedAt?: string | null;
}

export interface ConversationRunModeVm {
  mode: 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  autoConfig?: ConversationAutoConfigVm | null;
}

export interface ConversationAutoConfigVm {
  agentType: string;
  modelId?: string | null;
  permissionMode?: string | null;
  allowedProfiles?: string[];
  globalGoal?: string | null;
}

export interface ConversationCreateInput {
  projectId: string;
  content: string;
  runMode: 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  autoConfig?: ConversationAutoConfigVm | null;
  attachmentPaths?: string[];
}

export interface ConversationValidationResultVm {
  valid: boolean;
  missingItems: ConversationMissingItemVm[];
}

export interface ConversationMissingItemVm {
  code: string;
  label: string;
  recoveryPath: string;
}

export interface ConversationSearchResultVm {
  projectId: string;
  workspacePath: string;
  workspaceName: string;
  taskId: string;
  title: string;
  description?: string | null;
  requirementPreview: string;
  latestRun?: ConversationRunSummaryVm | null;
}

export interface AcpModelVm {
  id: string;
  name: string;
}
