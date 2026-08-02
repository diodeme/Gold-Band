export type DesktopThemePreference = 'system' | 'light' | 'light-gray' | 'dark' | 'black';
export type ConcreteDesktopTheme = Exclude<DesktopThemePreference, 'system'>;
export type DesktopThemeMode = 'light' | 'dark';
export type DesktopFontPreference = string;
export type DesktopLanguage = 'zh-cn' | 'en';
export type AvatarKind = 'agent' | 'user';
export type AvatarShape = 'circle' | 'square';
export type DesktopPlatform = 'macos' | 'windows' | 'linux' | 'unknown';
export type DesktopWindowFrameStyle = 'native-compositor' | 'app-outline';
export type UpdateCheckStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'not-available' | 'error';

export interface PreferencesVm {
  theme: DesktopThemePreference;
  language: DesktopLanguage;
  font: DesktopFontPreference;
  useLocalClaude: boolean;
  verboseLogging: boolean;
  avatars: AvatarPreferencesVm;
}

export interface AvatarImageVm {
  id: string;
  dataUrl: string;
  createdAt: string;
}

export interface AvatarProfileVm {
  shape: AvatarShape;
  selectedAvatarId: string | null;
  recentAvatars: AvatarImageVm[];
}

export interface AvatarPreferencesVm {
  agent: AvatarProfileVm;
  user: AvatarProfileVm;
}

export interface SaveDesktopAvatarInput {
  kind: AvatarKind;
  shape: AvatarShape;
  mimeType: string;
  dataBase64: string;
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

export interface MetricsSettingsVm {
  enabled: boolean;
  toggleLocked: boolean;
  metricsBaseUrl: string | null;
  heartbeatEndpoint: string | null;
  nodeMetricsEndpoint: string | null;
  apiKeySet: boolean;
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

export interface DesktopWindowChromeVm {
  frameStyle: DesktopWindowFrameStyle;
  nativeShadow: boolean;
}

export interface AppBootstrapVm {
  repoRoot: string;
  recentWorkspaces: string[];
  preferences: PreferencesVm;
  updaterSettings: UpdaterSettingsVm;
  metricsSettings: MetricsSettingsVm;
  updateStatus: UpdateStatusVm;
  updateBadges: UpdateBadgeStateVm;
  persistedAvailableUpdate?: UpdateInfoVm | null;
  clientVersion: string;
  platform: DesktopPlatform;
  windowChrome: DesktopWindowChromeVm;
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
  feedbackEnabled: boolean;
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
  primaryAgentDir: string;
  compatibleAgentDirs: string[];
  externalSessionSyncEnabled: boolean;
  supported: boolean;
  diagnostic?: ManagedAgentDiagnosticVm | null;
  supportedModes?: AcpModeVm[] | null;
  supportedModels?: AcpModeVm[] | null;
  configOptions?: AcpSelectConfigOptionVm[] | null;
  /** 是否支持 streamable HTTP MCP 传输（null=未诊断/未知） */
  mcpHttpSupported?: boolean | null;
  /** 是否支持 SSE MCP 传输（null=未诊断/未知） */
  mcpSseSupported?: boolean | null;
}

export interface AcpModeVm {
  id: string;
  name: string;
  description?: string | null;
}

export interface AcpSelectConfigValueVm {
  value: string;
  name: string;
  description?: string | null;
}

export interface AcpSelectConfigOptionVm {
  id: string;
  category?: string | null;
  name?: string | null;
  description?: string | null;
  currentValue?: string | null;
  options: AcpSelectConfigValueVm[];
}

export interface AcpCommandItemVm {
  name: string;
  description: string;
  inputHint?: string | null;
}

export interface AcpCommandCatalogVm {
  agentType: string;
  workspaceKey: string;
  commands: AcpCommandItemVm[];
  updatedAt: string;
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
  primaryAgentDir: string;
  compatibleAgentDirs: string[];
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
  primaryAgentDir: string;
  compatibleAgentDirs: string[];
  externalSessionSyncEnabled: boolean;
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
  model?: string | null;
}

export interface WorkflowAiDynamicFixedAgentStrategyDsl {
  mode: 'fixed';
  provider: string;
  model?: string;
}

export interface WorkflowAiDynamicDynamicAgentStrategyDsl {
  mode: 'dynamic';
  bootstrapProvider: string;
  bootstrapModel?: string | null;
  acceptanceModel?: string | null;
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
  new_round_entry?: '$entry' | string | null;
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

export interface AutoTemplateStore {
  version: string;
  templates: AutoTemplate[];
}

export interface AutoTemplate {
  id: string;
  name: string;
  config: ConversationAutoConfigVm;
  createdAt: string;
  updatedAt: string;
}

export type ProfileScope = 'built-in' | 'user';

export interface ProfileVm {
  id: string;
  name: string;
  summary: string;
  summarySource?: string;
  content: string;
  dynamicTemplate: boolean;
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
  name: string;
  summary: string;
  content: string;
  dynamicTemplate: boolean;
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

export interface RuntimeDisplayVm {
  code: string;
  tone: 'success' | 'danger' | 'running' | 'warning' | 'neutral' | string;
  icon: 'check' | 'error' | 'pause' | 'dot' | string;
  terminal: boolean;
  resumable: boolean;
  reasonCode?: string | null;
  blockingError: boolean;
}

export interface GraphNodeVm {
  id: string;
  nodeId?: string | null;
  sequence?: number | null;
  label: string;
  nodeType: string;
  status?: string | null;
  outcome?: string | null;
  runtimeDisplay: RuntimeDisplayVm;
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
  runtimeDisplay: RuntimeDisplayVm;
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
  branchId: string;
  parentBranchId?: string | null;
  readOnly: boolean;
  branchExecution?: AcpAgentExecutionVm | null;
  sessionId?: string | null;
  title?: string | null;
  roundId?: string | null;
  nodeId?: string | null;
  attemptId?: string | null;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  provider: string;
  adapterId?: string | null;
  adapterDisplayName?: string | null;
  adapterIconKey?: string | null;
  cwd?: string | null;
  providerCwd?: string | null;
  status: string;
  sessionStartedAt?: string | null;
  sessionUpdatedAt?: string | null;
  sessionElapsedSeconds?: number | null;
  timing?: AcpSessionTimingVm | null;
  restored: boolean;
  stopReason?: string | null;
  systemPromptAppend?: string | null;
  config?: AcpSessionConfigVm | null;
  events: AcpUiEventVm[];
  eventPage: AcpEventPageVm;
  timelineProjection: AcpTimelineProjectionVm | null;
  pendingPermissions: AcpPermissionRequestVm[];
  availableCommands?: unknown[] | null;
  usage?: AcpUsageVm | null;
  diagnostics: AcpDiagnosticsVm;
}

export interface ActiveSessionStopVm {
  kind: 'run-paused' | 'session-cancelled' | string;
  run?: RunSummaryVm | null;
  session?: AcpSessionVm | null;
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export interface AcpSessionQueryInput {
  branchId?: string;
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
  modelOverrideId?: string | null;
  permissionModeOverrideId?: string | null;
  configOptionOverrides?: Record<string, string>;
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
  timing?: AcpTimingPatchVm | null;
  raw?: unknown;
}

export interface AcpTimingPatchVm {
  sessionElapsedSeconds: number;
  revision?: number | null;
  observedAt?: string | null;
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
  userWaitStartedAt?: string | null;
  waitReason?: string | null;
  paused: boolean;
  reason?: string | null;
}

export interface AcpSessionTimingVm {
  sessionElapsedSeconds: number;
  revision?: number | null;
  observedAt?: string | null;
  activeTurnStartedAt?: string | null;
  activeTurnLastActivityAt?: string | null;
  permissionWaitStartedAt?: string | null;
  userWaitStartedAt?: string | null;
  waitReason?: string | null;
  paused: boolean;
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

// Navigation payload emitted after clicking "View details" in a system toast.
// It carries the complete attempt locator and a deduplication key.
export interface InterventionNavigateEventVm {
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  dedupKey: string;
}

export interface NotificationAttentionInput {
  windowFocused: boolean;
  windowMinimized: boolean;
  windowVisible: boolean;
  projectId?: string | null;
  taskId?: string | null;
  runId?: string | null;
  roundId?: string | null;
  nodeId?: string | null;
  attemptId?: string | null;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}


export interface AcpDiagnosticsVm {
  rawFrameCount: number;
  eventCount: number;
  errorCount: number;
  lastError?: string | null;
  lastErrorTimestamp?: string | null;
}

export type AcpRawFrameOrder = "asc" | "desc";

export interface AcpRawFrameQueryInput {
  page?: number;
  pageSize?: number;
  search?: string;
  kind?: string;
  direction?: string;
  order?: AcpRawFrameOrder;
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
  order: AcpRawFrameOrder;
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

// Conversation UI types

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
  runMode: 'direct' | 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  agentIdentity?: ConversationAgentIdentityVm | null;
  lastActivityAt?: string | null;
  activity?: ConversationTaskActivityVm | null;
  latestRun?: ConversationRunSummaryVm | null;
  runs: ConversationRunSummaryVm[];
  pinned: boolean;
  pinnedOrder?: number | null;
}

export interface AcpActivityDetailQueryInput {
  branchId: string;
  activityStartSeq: number;
  activityEndSeq: number;
  earlierCursor?: string | null;
  limit?: number;
}

export interface AcpActivityDetailVm {
  items: AcpUiEventVm[];
  hasMoreEarlier: boolean;
  earlierCursor?: string | null;
}

export interface AcpToolDetailQueryInput {
  branchId: string;
  eventId: string;
  toolCallId?: string | null;
}

export interface AcpToolDetailVm {
  event?: AcpUiEventVm | null;
}

export interface AcpTimelineProjectionVm {
  agents: AcpAgentExecutionVm[];
  todoEntries: Array<{ content?: string; status?: string; priority?: string }>;
}

export interface AcpAgentExecutionVm {
  agentExecutionId: string;
  parentAgentExecutionId?: string | null;
  attemptId?: string | null;
  executionStatus: string;
  eventCount: number;
  toolCallCount: number;
  readFileCount: number;
  writtenFileCount: number;
  hasAttention: boolean;
  title?: string | null;
  description?: string | null;
  todoEntries: Array<{ content?: string; status?: string; priority?: string }>;
}

export interface ConversationTaskActivityVm {
  phase: string;
  stopping: boolean;
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

export interface ConversationRuntimeFacetVm {
  status: string;
  outcome?: string | null;
  pauseReason?: string | null;
  resumable: boolean;
  current: boolean;
  active: boolean;
  continuable: boolean;
  phase: string;
}

export interface ConversationAcpFacetVm {
  status?: string | null;
  phase?: 'starting' | 'running' | 'cancel-requested' | null;
  active: boolean;
  stopping: boolean;
  terminal: boolean;
}

export interface ConversationComposerVm {
  mode: 'normal' | 'runtime-active' | 'stopping' | 'interrupted-input' | 'invalid-workflow' | 'runtime-error' | 'permission-blocked' | 'submitting' | string;
  submitTarget: 'acp-prompt' | 'runtime-continue' | 'permission-response' | 'none' | string;
  processingKind: 'sending' | 'launching' | 'processing' | 'thinking' | 'tool' | 'compacting' | 'responding' | 'stopping' | 'launching-next-node' | string;
  statusKey?: string | null;
  canStop: boolean;
  lockInput: boolean;
}

export interface ConversationAttemptLifecycleVm {
  runtime: ConversationRuntimeFacetVm;
  acp: ConversationAcpFacetVm;
  displayStatus: string;
  runtimeDisplay: RuntimeDisplayVm;
  continueKind?: 'input' | null;
  composer: ConversationComposerVm;
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
  runtimeDisplay: RuntimeDisplayVm;
  lifecycle?: ConversationAttemptLifecycleVm | null;
  current: boolean;
  manualCheckPending: boolean;
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
  runtimeDisplay: RuntimeDisplayVm;
  nodes: ConversationTreeNodeVm[];
}

export interface ConversationTreeNodeVm {
  nodeId: string;
  label: string;
  nodeType: string;
  status: string;
  runtimeDisplay: RuntimeDisplayVm;
  attempts: ConversationSessionLeafVm[];
  outerNodes?: ConversationTreeNodeVm[];
}

export interface ConversationRunVm {
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
  title: string;
  autoTitle: boolean;
  runMode: 'direct' | 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  directConfig?: ConversationDirectConfigVm | null;
  agentIdentity?: ConversationAgentIdentityVm | null;
  lastActivityAt?: string | null;
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
  runtimeErrorMessage?: string | null;
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
  runtimeDisplay: RuntimeDisplayVm;
  lifecycle?: ConversationAttemptLifecycleVm | null;
  manualCheckPending: boolean;
  sessionId?: string | null;
  startedAt?: string | null;
}

export interface ConversationRunModeVm {
  mode: 'direct' | 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  includeInterview?: boolean | null;
  directConfig?: ConversationDirectConfigVm | null;
  directPreferences?: Record<string, ConversationDirectConfigVm>;
  autoConfig?: ConversationAutoConfigVm | null;
}

export interface ConversationDirectConfigVm {
  agentType: string;
  modelId?: string | null;
  permissionMode?: string | null;
  configOptions?: Record<string, string>;
}

export interface ConversationAgentIdentityVm {
  agentType: string;
  displayName: string;
  iconKey: string;
}

export interface ConversationAutoConfigVm {
  agentStrategy?: 'fixed' | 'dynamic';
  agentType: string;
  bootstrapAgentType?: string | null;
  bootstrapModelId?: string | null;
  acceptanceModelId?: string | null;
  modelId?: string | null;
  permissionMode?: string | null;
  configOptions?: Record<string, string>;
  availableAgents?: DynamicAgentRefDsl[];
  routingPrompt?: string | null;
  allowedWorkflows?: AllowedWorkflowRefDsl[];
  allowedProfiles?: string[];
  globalGoal?: string | null;
  control?: DynamicControlDsl | null;
  activeTemplateId?: string | null;
  activeTemplateName?: string | null;
}

export interface ConversationCreateInput {
  projectId: string;
  content: string;
  runMode: 'direct' | 'auto' | 'workflow';
  workflowTemplateId?: string | null;
  includeInterview?: boolean | null;
  directConfig?: ConversationDirectConfigVm | null;
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
  matchPreview: string;
  latestRun?: ConversationRunSummaryVm | null;
  runMode: 'direct' | 'auto' | 'workflow';
  agentIdentity?: ConversationAgentIdentityVm | null;
  lastActivityAt?: string | null;
}

export interface AcpModelVm {
  id: string;
  name: string;
}

// MCP and skill types

export interface McpServerHealthResult {
  status: 'healthy' | 'unhealthy' | 'auth_required' | 'unknown';
  message?: string | null;
  authUrl?: string | null;
  needsClientSecret?: boolean | null;
}

export interface McpServerVm {
  id: string;
  name: string;
  enabled: boolean;
  transport: 'stdio' | 'http' | 'sse';
  command?: string | null;
  args?: string[] | null;
  env?: AgentEnvEntryVm[] | null;
  url?: string | null;
  headers?: AgentEnvEntryVm[] | null;
  managed: boolean;
  helpMessage?: string | null;
  healthStatus?: 'healthy' | 'unhealthy' | 'auth_required' | 'stopped' | 'checking' | 'unknown' | null;
  healthMessage?: string | null;
}

export interface ToolInfo {
  name: string;
  description?: string | null;
  inputSchema?: Record<string, unknown> | null;
}

export interface SkillMetaVm {
  name: string;
  description: string;
  source: 'built-in' | 'global' | 'project';
  directoryPath: string;
  agentSource: string;
  loadWarnings: string[];
  syncedAgentTypes: string[];
}

export interface SyncStatusEntryVm {
  agentType: string;
  isSynced: boolean;
}

export interface SkillListVm {
  global: SkillMetaVm[];
  project: SkillMetaVm[];
}

export interface SkillContentVm {
  meta: SkillMetaVm;
  descriptionSource?: string;
  body: string;
}

// -- Feedback --
export interface FeedbackScreenshotInput { name: string; mime: string; size: number; dataBase64: string; }
export interface FeedbackInput { description: string; projectId?: string | null; taskId?: string | null; screenshots: FeedbackScreenshotInput[]; includeLogs: boolean; }
export interface FeedbackResult { success: boolean; }
export interface FeedbackArchivePreview {
  uncompressedBytes: number;
  fileCount: number;
  withinLimits: boolean;
  maxUncompressedBytes: number;
  maxFileCount: number;
}

