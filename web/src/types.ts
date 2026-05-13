export type DesktopThemePreference = 'system' | 'light' | 'light-warm' | 'dark' | 'black';
export type ConcreteDesktopTheme = Exclude<DesktopThemePreference, 'system'>;
export type DesktopThemeMode = 'light' | 'dark';
export type DesktopFontPreference = string;
export type DesktopLanguage = 'zh-cn' | 'en';

export interface PreferencesVm {
  theme: DesktopThemePreference;
  language: DesktopLanguage;
  font: DesktopFontPreference;
}

export interface AppBootstrapVm {
  repoRoot: string;
  recentWorkspaces: string[];
  preferences: PreferencesVm;
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
  workflowError?: string | null;
  latestRun?: RunSummaryVm | null;
  resumableRunId?: string | null;
  artifactCount: number;
  attachmentCount: number;
}

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

export interface WorkflowControlVm {
  maxRepairLoops: number;
  maxAcceptanceLoops: number;
  onAcceptanceFailure: string;
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
  requirement: string;
  selectedNodeDetail?: NodeDetailVm | null;
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
  repairLoopsUsed: number;
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
  artifactCount: number;
  attachmentCount: number;
  current: boolean;
}

export interface GraphEdgeVm {
  from: string;
  to: string;
  label: string;
}

export interface NodeDetailVm {
  id: string;
  nodeId: string;
  sequence?: number | null;
  label: string;
  nodeType: string;
  status: string;
  outcome?: string | null;
  attemptId: string;
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
  acpSession?: AcpSessionVm | null;
}

export interface AcpSessionVm {
  sessionId?: string | null;
  provider: string;
  adapterId?: string | null;
  adapterDisplayName?: string | null;
  cwd?: string | null;
  status: string;
  restored: boolean;
  stopReason?: string | null;
  config?: AcpSessionConfigVm | null;
  events: AcpUiEventVm[];
  eventPage: AcpEventPageVm;
  pendingPermissions: AcpPermissionRequestVm[];
  availableCommands?: unknown[] | null;
  usage?: unknown | null;
  diagnostics: AcpDiagnosticsVm;
}

export interface AcpSessionQueryInput {
  beforeSeq?: number;
  afterSeq?: number;
  eventLimit?: number;
}

export interface AcpEventPageVm {
  loadedCount: number;
  total: number;
  oldestSeq?: number | null;
  newestSeq?: number | null;
  hasOlder: boolean;
  hasNewer: boolean;
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
  nodeId: string;
  attemptId: string;
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

export type PrimaryModule = 'task-orchestration' | 'knowledge-base' | 'model-management' | 'settings';

export type TaskPage =
  | { kind: 'task-list' }
  | { kind: 'workflow'; taskId: string }
  | { kind: 'round-detail'; taskId: string; runId: string; roundId: string };

type RoundSelectionContext = { contextNodeId?: string };

export type RoundSelection = RoundSelectionContext & (
  | { kind: 'round' }
  | { kind: 'requirement' }
  | { kind: 'node'; nodeId: string }
  | { kind: 'artifact'; nodeId: string; attemptId: string; name: string }
  | { kind: 'attachment'; nodeId: string; attemptId: string; name: string }
  | { kind: 'worker-ref'; nodeId: string; attemptId: string }
  | { kind: 'event'; id: string; nodeId?: string; attemptId?: string }
  | { kind: 'log'; id: string; nodeId?: string; attemptId?: string }
);
