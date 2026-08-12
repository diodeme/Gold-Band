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
  turnFiles: TurnFilesVm;
  workspaceLayout: WorkspaceLayoutVm;
  workspaceFiles: WorkspaceFilesVm;
}

export interface TurnFilesVm {
  cardPreviewLimit: number;
}

export interface WorkspaceFilesVm {
  autoSaveDelayMs: number;
  searchDebounceMs: number;
  searchResultLimit: number;
  textEditableMaxBytes: number;
  textHighlightMaxChars: number;
  textReadOnlyMaxBytes: number;
  imagePreviewMaxBytes: number;
  imagePreviewMaxPixels: number;
  contentCacheEntries: number;
  contentCacheMaxBytes: number;
  watchDebounceMs: number;
  externalAccessGrantTtlSeconds: number;
  markdownLivePreviewMaxChars: number;
  markdownEmbeddedImageLimit: number;
  markdownEmbeddedImageMaxConcurrent: number;
}

export interface FileRevisionVm {
  byteLength: number;
  modifiedAtNs: string;
  contentHash: string;
}

export interface WorkspaceFileLocatorVm {
  projectId: string;
  canonicalPath: string;
  relativePath: string | null;
  scope: 'workspace' | 'external';
}

export interface ExternalFileAccessGrantVm {
  token: string;
  permissions: Array<'read' | 'write'>;
  expiresAtMs: string;
}

export interface FileTargetLocationVm {
  line: number | null;
  column: number | null;
  endLine: number | null;
}

export interface ResolvedWorkspaceFileLinkVm {
  locator: WorkspaceFileLocatorVm;
  target: FileTargetLocationVm | null;
  externalAccessGrant: ExternalFileAccessGrantVm | null;
}

export interface WorkspaceDirectoryEntryVm {
  name: string;
  relativePath: string;
  canonicalPath: string;
  kind: 'directory' | 'file' | 'symlink' | 'other';
  hasChildren: boolean;
  byteLength: number | null;
  modifiedAtNs: string | null;
}

export interface WorkspaceFileSearchVm {
  requestId: string;
  entries: WorkspaceDirectoryEntryVm[];
  truncated: boolean;
}

interface WorkspaceFileSnapshotBaseVm {
  locator: WorkspaceFileLocatorVm;
  name: string;
  revision: FileRevisionVm;
  externalAccessGrant: ExternalFileAccessGrantVm | null;
}

export interface TextFileSnapshotVm extends WorkspaceFileSnapshotBaseVm {
  kind: 'text';
  content: string;
  encoding: string;
  language: string | null;
  lineEnding: 'lf' | 'crlf' | 'mixed';
  editable: boolean;
  limitationCode: string | null;
}

export interface WorkspaceFilePreviewGrantVm {
  token: string;
  expiresAtMs: string;
}

export interface ImageFileSnapshotVm extends WorkspaceFileSnapshotBaseVm {
  kind: 'image';
  mimeType: string;
  width: number;
  height: number;
  animated: boolean;
  previewGrant: WorkspaceFilePreviewGrantVm;
  sourceEditable: boolean;
}

export interface UnsupportedFileSnapshotVm extends WorkspaceFileSnapshotBaseVm {
  kind: 'unsupported';
  mimeType: string | null;
  limitationCode: string;
}

export type WorkspaceFileSnapshotVm =
  | TextFileSnapshotVm
  | ImageFileSnapshotVm
  | UnsupportedFileSnapshotVm;

export interface ResolveMarkdownImageInput {
  projectId: string;
  markdownCanonicalPath: string;
  markdownExternalAccessToken: string | null;
  rawSrc: string;
  approvedExternalTargets: string[];
}

export type MarkdownImagePreviewVm =
  | {
      kind: 'ready';
      canonicalPath: string;
      previewGrant: WorkspaceFilePreviewGrantVm;
      mimeType: string;
      width: number;
      height: number;
      animated: boolean;
    }
  | {
      kind: 'approvalRequired';
      canonicalPath: string;
      reason: 'outside-document-directory';
    }
  | {
      kind: 'unsupported';
      limitationCode: string;
    };

export interface WriteFileResourceInput {
  projectId: string;
  canonicalPath: string;
  externalAccessToken: string | null;
  content: string;
  encoding: string;
  lineEnding: string;
  expectedRevision: FileRevisionVm;
  operationId: string;
  force: boolean;
}

export interface WorkspaceFileChangedEventVm {
  projectId: string;
  canonicalPath: string;
  kind: 'created' | 'modified' | 'removed' | 'renamed';
  revision: FileRevisionVm | null;
  operationId: string | null;
}

export interface WorkspaceLayoutVm {
  shellMinWidth: number;
  shellMinHeight: number;
  rightWorkspace: RightWorkspaceLayoutVm;
  conversation: WorkspaceLayoutProfileVm;
  contextCards: WorkspaceLayoutProfileVm;
  workflowCanvas: WorkspaceLayoutProfileVm;
  settings: WorkspaceLayoutProfileVm;
}

export interface FileWorkspaceLayoutVm {
  preferredWidth: number;
  splitMinWidth: number;
  treeDefaultWidth: number;
  treeMinWidth: number;
  treeMaxWidth: number;
}

export interface RightWorkspaceLayoutVm {
  minWidth: number;
  defaultWidth: number;
  maxWidth: number;
  file: FileWorkspaceLayoutVm;
}

export interface WorkspaceLayoutProfileVm {
  centerMinWidth: number;
  centerAutoCollapseWidth: number;
  windowMinWidth: number;
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
  catalog: AgentCatalogEntryVm[];
}

export interface ManagedAgentVm {
  agentType: string;
  displayName: string;
  command: string;
  args: string[];
  env: AgentEnvEntryVm[];
  iconKey: string;
  primaryAgentDir: string;
  projectPrimaryAgentDir: string | null;
  compatibleAgentDirs: string[];
  supportsSystemPrompt: boolean;
  externalSessionSyncSupported: boolean;
  externalSessionSyncEnabled: boolean;
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

export interface AgentCatalogEntryVm {
  agentType: string;
  label: string;
  iconKey: string;
  version: string;
  description: string;
  repository?: string | null;
  website?: string | null;
  primaryAgentDir: string;
  projectPrimaryAgentDir: string | null;
  compatibleAgentDirs: string[];
  configured: boolean;
  supportsSystemPrompt: boolean;
  supportsExternalSessionSync: boolean;
  defaultDisplayName: string;
  defaultCommand: string;
  defaultArgs: string[];
  defaultEnv: AgentEnvEntryVm[];
}

export interface ManagedAgentInput {
  displayName: string;
  icon: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  primaryAgentDir: string;
  projectPrimaryAgentDir: string | null;
  compatibleAgentDirs: string[];
  externalSessionSyncSupported: boolean;
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

export interface GitCapabilityVm {
  status: 'ready' | 'not-installed' | 'repository-required' | 'head-required' | 'worktree-required' | 'repository-unavailable';
  repoRoot: string | null;
  commonDir: string | null;
  head: string | null;
}

export type GitLockOwnerVm = 'user' | 'runtime';

export interface GitLockVm {
  locked: boolean;
  owner?: GitLockOwnerVm | null;
  operation?: string | null;
}

export interface GitUpstreamVm {
  name: string;
  ahead: number;
  behind: number;
}

export interface GitRemoteVm {
  name: string;
  fetchUrls: string[];
  pushUrls: string[];
}

export interface GitRepositorySnapshotVm {
  projectId: string;
  repoRoot: string;
  commonDir: string;
  workspacePath: string;
  headOid?: string | null;
  currentBranch?: string | null;
  detached: boolean;
  unborn: boolean;
  upstream?: GitUpstreamVm | null;
  remotes: GitRemoteVm[];
  lock: GitLockVm;
  revision: string;
}

export type GitFileChangeKindVm =
  | 'added'
  | 'modified'
  | 'deleted'
  | 'renamed'
  | 'copied'
  | 'type-changed'
  | 'unmerged'
  | 'untracked';

export interface GitFileChangeVm {
  path: string;
  oldPath?: string | null;
  kind: GitFileChangeKindVm;
  indexStatus?: string | null;
  worktreeStatus?: string | null;
  binary: boolean;
  submodule: boolean;
  addedLines?: number | null;
  deletedLines?: number | null;
}

export interface GitBranchStatusVm {
  oid?: string | null;
  head?: string | null;
  upstream?: string | null;
  ahead: number;
  behind: number;
}

export interface GitWorkspaceStatusVm {
  snapshotRevision: string;
  branch: GitBranchStatusVm;
  conflicts: GitFileChangeVm[];
  staged: GitFileChangeVm[];
  unstaged: GitFileChangeVm[];
  untracked: GitFileChangeVm[];
  operationInProgress?: {
    kind: 'merge' | 'rebase' | 'cherry-pick' | 'revert';
    currentOid?: string | null;
    currentSubject?: string | null;
  } | null;
}

export type GitRefKindVm = 'local-branch' | 'remote-branch' | 'tag';

export interface GitRefVm {
  fullName: string;
  shortName: string;
  kind: GitRefKindVm;
  targetOid: string;
  peeledOid?: string | null;
  upstream?: string | null;
  ahead?: number | null;
  behind?: number | null;
  checkedOutWorktreePaths: string[];
}

export interface GitWorktreeVm {
  path: string;
  headOid: string;
  branch?: string | null;
  main: boolean;
  detached: boolean;
  locked: boolean;
  lockReason?: string | null;
  prunable: boolean;
  ownership: 'user' | 'runtime';
  runtimeStatus?: string | null;
}

export interface GitSignatureVm {
  name: string;
  email?: string | null;
  timestamp: string;
}

export interface GitStashEntryVm {
  refName: string;
  oid: string;
  baseOid: string;
  message: string;
  author: GitSignatureVm;
  createdAt: string;
}

export interface GitRefLabelVm {
  fullName: string;
  shortName: string;
  kind: GitRefKindVm;
}

export interface GitCommitVm {
  oid: string;
  parentOids: string[];
  subject: string;
  body: string;
  author: GitSignatureVm;
  committer: GitSignatureVm;
  refs: GitRefLabelVm[];
  sourceRef?: string | null;
  runtimeCheckpoint: boolean;
}

export interface GitHistoryQueryVm {
  cursor?: string | null;
  limit?: number | null;
  revision?: string | null;
  refName?: string | null;
}

export interface GitHistoryPageVm {
  commits: GitCommitVm[];
  nextCursor?: string | null;
  revision: string;
}

export interface GitCommitFileChangeVm {
  path: string;
  oldPath?: string | null;
  kind: GitFileChangeKindVm;
  binary: boolean;
  addedLines?: number | null;
  deletedLines?: number | null;
}

export interface GitCommitDetailVm {
  commit: GitCommitVm;
  files: GitCommitFileChangeVm[];
}

export interface GitCommitReviewQueryVm {
  selectedOids: string[];
  revision?: string | null;
}

export interface GitCommitReviewFileVm {
  path: string;
  oldPath?: string | null;
  kind: GitFileChangeKindVm;
  binary: boolean;
  beforeOid?: string | null;
  beforePath?: string | null;
  afterOid: string;
}

export interface GitCommitReviewVm {
  selectedOids: string[];
  revision: string;
  files: GitCommitReviewFileVm[];
  totals: {
    commitCount: number;
    fileCount: number;
  };
}

export interface GitCommitReachabilityQueryVm {
  oid: string;
  targetRef: string;
}

export interface GitCommitReachabilityVm {
  oid: string;
  containingRefs: GitRefLabelVm[];
  targetRef: string;
  targetOid: string;
  targetPath: 'tip' | 'direct' | 'merged' | 'not-contained';
  firstMergeOid?: string | null;
  parentOids: string[];
}

export interface GitSourceControlSnapshotVm {
  repository: GitRepositorySnapshotVm;
  status: GitWorkspaceStatusVm;
  refs: GitRefVm[];
  worktrees: GitWorktreeVm[];
  stashes: GitStashEntryVm[];
}

export type GitMutationVm =
  | { kind: 'stage-paths'; paths: string[] }
  | { kind: 'stage-all' }
  | { kind: 'unstage-paths'; paths: string[] }
  | { kind: 'unstage-all' }
  | { kind: 'commit'; subject: string; body?: string | null }
  | { kind: 'branch-create'; name: string; startPoint?: string | null; checkout: boolean }
  | { kind: 'branch-switch'; name: string }
  | { kind: 'branch-rename'; oldName?: string | null; newName: string }
  | { kind: 'branch-delete-safe'; name: string }
  | { kind: 'tag-create'; name: string; target?: string | null; style: 'annotated' | 'lightweight'; message?: string | null }
  | { kind: 'tag-delete-local'; name: string }
  | { kind: 'worktree-create'; path: string; sourceRef: string; newBranch?: string | null }
  | { kind: 'worktree-remove'; path: string };

export type GitMutationRequestVm = GitMutationVm & {
  expectedRevision?: string | null;
};

export type GitMutationResultVm =
  | {
    scope: 'workspace';
    status: GitWorkspaceStatusVm;
    repositoryRevision: string;
  }
  | { scope: 'repository' };

export type GitPullStrategyVm = 'fast-forward-only' | 'merge' | 'rebase';

export type GitOperationInputVm =
  | { kind: 'fetch'; remote?: string | null; prune: boolean }
  | { kind: 'pull'; remote?: string | null; branch?: string | null; strategy: GitPullStrategyVm }
  | { kind: 'push'; remote: string; branch: string; setUpstream: boolean }
  | { kind: 'push-tag'; remote: string; tag: string }
  | { kind: 'stash-create'; message?: string | null; includeUntracked: boolean }
  | { kind: 'stash-apply'; stashRef: string; restoreIndex: boolean }
  | { kind: 'merge-continue' }
  | { kind: 'merge-abort' }
  | { kind: 'rebase-continue' }
  | { kind: 'rebase-skip' }
  | { kind: 'rebase-abort' };

export type GitOperationRequestVm = GitOperationInputVm & {
  expectedRevision?: string | null;
};

export type GitOperationStatusVm =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'conflicted';

export interface GitOperationErrorVm {
  code: string;
  params: Record<string, unknown>;
}

export interface GitOperationVm {
  operationId: string;
  kind: GitOperationInputVm['kind'];
  repositoryCommonDir: string;
  workspacePath?: string | null;
  status: GitOperationStatusVm;
  cancelable: boolean;
  startedAt?: string | null;
  completedAt?: string | null;
  error?: GitOperationErrorVm | null;
}

export interface GitStateChangedEventVm {
  projectId: string;
  repositoryCommonDir: string;
  workspacePath: string;
  reason: 'workspace' | 'metadata' | 'operation';
}

export type GitHubCapabilityStatusVm = 'not-installed' | 'not-authenticated' | 'repository-unresolved' | 'ready';

export interface GitHubCapabilityVm {
  status: GitHubCapabilityStatusVm;
  version?: string | null;
  host?: string | null;
  account?: string | null;
  repository?: string | null;
  remote?: string | null;
  defaultBranch?: string | null;
}

export interface GitHubOperationVm {
  operationId: string;
  kind: 'login' | 'pr-create';
  host: string;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  cancelable: boolean;
  startedAt?: string | null;
  completedAt?: string | null;
  error?: { code: string; params: Record<string, unknown> } | null;
  resultUrl?: string | null;
}

export interface GitHubPullRequestPreflightInputVm {
  host: string;
  repository: string;
  head: string;
  base: string;
}

export interface GitHubPullRequestCreateInputVm extends GitHubPullRequestPreflightInputVm {
  title: string;
  body: string;
  draft: boolean;
}

export interface GitHubPullRequestPreflightVm {
  remote: string;
  head: string;
  base: string;
  aheadBy: number;
  headPublished: boolean;
  existingPullRequest?: GitHubPullRequestSummaryVm | null;
}

export interface GitHubActorVm { login: string; name?: string | null }
export interface GitHubLabelVm { name: string; color?: string | null }
export interface GitHubStatusCheckVm {
  kind?: string | null;
  name?: string | null;
  context?: string | null;
  state?: string | null;
  status?: string | null;
  conclusion?: string | null;
}

export interface GitHubPullRequestSummaryVm {
  number: number;
  title: string;
  state: string;
  draft: boolean;
  author?: GitHubActorVm | null;
  headRefName: string;
  baseRefName: string;
  updatedAt: string;
  url: string;
  reviewDecision?: string | null;
  labels: GitHubLabelVm[];
  statusChecks: GitHubStatusCheckVm[];
}

export interface GitHubPullRequestDetailVm extends GitHubPullRequestSummaryVm {
  baseRefOid: string;
  headRefOid: string;
  body: string;
  mergeable?: string | null;
  mergeStateStatus?: string | null;
  additions: number;
  deletions: number;
  changedFiles: number;
  files: Array<{ path: string; additions: number; deletions: number }>;
  latestReviews: Array<{ author?: GitHubActorVm | null; state: string }>;
}

export interface GitHubIssueSummaryVm {
  number: number;
  title: string;
  state: string;
  author?: GitHubActorVm | null;
  assignees: GitHubActorVm[];
  labels: GitHubLabelVm[];
  updatedAt: string;
  url: string;
}

export interface GitHubIssueDetailVm extends GitHubIssueSummaryVm {
  body: string;
  milestone?: { title: string } | null;
}

export type GitHubListStateVm = 'open' | 'closed' | 'all';
export interface GitHubPullRequestQueryVm {
  state: GitHubListStateVm;
  author?: string | null;
  base?: string | null;
  head?: string | null;
  label?: string | null;
  search?: string | null;
}
export interface GitHubIssueQueryVm {
  state: GitHubListStateVm;
  author?: string | null;
  assignee?: string | null;
  label?: string | null;
  milestone?: string | null;
  search?: string | null;
}

export type GitComparisonSourceVm =
  | { kind: 'workspace'; workspacePath?: string | null; path: string; area: 'staged' | 'unstaged' }
  | { kind: 'commit'; workspacePath?: string | null; path: string; beforeOid?: string | null; beforePath?: string | null; afterOid: string }
  | { kind: 'github-pr'; workspacePath?: string | null; host: string; repository: string; prNumber: number; baseOid: string; headOid: string; path: string };

export interface GitFileComparisonVm {
  path: string;
  stats: { addedLines: number; deletedLines: number };
  before?: { content: string } | null;
  after?: { content: string } | null;
  limitationCode?: string | null;
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
  config_options?: Record<string, string>;
  manual_check?: boolean | null;
}

export type WorkflowAiDynamicAgentStrategyDsl = WorkflowAiDynamicFixedAgentStrategyDsl | WorkflowAiDynamicDynamicAgentStrategyDsl;

export interface DynamicAgentRefDsl {
  provider: string;
  model?: string | null;
  permissionMode?: string | null;
  configOptions?: Record<string, string>;
}

export interface WorkflowAiDynamicFixedAgentStrategyDsl {
  mode: 'fixed';
  provider: string;
  model?: string;
  permissionMode?: string | null;
}

export interface WorkflowAiDynamicDynamicAgentStrategyDsl {
  mode: 'dynamic';
  bootstrapProvider: string;
  bootstrapModel?: string | null;
  permissionMode?: string | null;
  bootstrapConfigOptions?: Record<string, string>;
  acceptanceModel?: string | null;
  acceptanceConfigOptions?: Record<string, string>;
  routingPrompt: string;
  availableAgents: DynamicAgentRefDsl[];
}

export interface WorkflowAiDynamicNodeDsl {
  type: 'ai-dynamic';
  id: string;
  agentStrategy: WorkflowAiDynamicAgentStrategyDsl;
  configOptions?: Record<string, string>;
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

export interface ImportProfilesInput {
  folderPath: string;
  dynamicTemplate: boolean;
}

export type ImportRecordStatus =
  | 'imported'
  | 'imported-with-fallbacks'
  | 'failed';

export type ProfileFieldFallback =
  | 'name'
  | 'summary'
  | 'frontmatter-missing'
  | 'dynamic-template-downgraded';

export type ImportProfileErrorCode =
  | 'read-failed'
  | 'invalid-frontmatter'
  | 'empty-file'
  | 'missing-name'
  | 'create-failed';

export interface ImportProfileError {
  code: ImportProfileErrorCode;
}

export interface ImportedProfileRecord {
  sourcePath: string;
  status: ImportRecordStatus;
  name: string;
  fallbacks: ProfileFieldFallback[];
  importedId: string | null;
  error: ImportProfileError | null;
}

export interface ImportProfilesResult {
  totalScanned: number;
  imported: ImportedProfileRecord[];
  failed: ImportedProfileRecord[];
  truncated: boolean;
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
  pendingElicitations: AcpElicitationRequestVm[];
  availableCommands?: unknown[] | null;
  usage?: AcpUsageVm | null;
  diagnostics: AcpDiagnosticsVm;
}

export interface ActiveSessionStopVm {
  operationId: string;
  status: 'accepted' | string;
  kind: 'stop-accepted' | string;
  run?: RunSummaryVm | null;
  session?: AcpSessionVm | null;
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export interface AcpSessionQueryInput {
  traceId?: string;
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

export interface TurnFileLocatorVm {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  branchId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}

export type FileChangeKindVm = 'added' | 'modified' | 'deleted' | 'renamed';

export interface FileVersionRefVm {
  id: string;
  storageKind: 'capturedBlob';
  contentHash: string;
  byteLength: number;
  encoding?: string | null;
  lineEnding?: string | null;
}

export interface TurnFileChangeVm {
  id: string;
  changeKind: FileChangeKindVm;
  logicalPath: string;
  previousLogicalPath?: string | null;
  mimeType?: string | null;
  text: boolean;
  addedLines?: number | null;
  deletedLines?: number | null;
  beforeVersion?: FileVersionRefVm | null;
  afterVersion?: FileVersionRefVm | null;
  limitationCode?: string | null;
}

export interface TurnFileChangeSummaryVm {
  fileCount: number;
  addedFiles: number;
  modifiedFiles: number;
  deletedFiles: number;
  addedLines: number;
  deletedLines: number;
}

export interface TurnFileChangeSetVm {
  schemaVersion?: number;
  id: string;
  turnId: string;
  promptEventId: string;
  branchId: string;
  status: 'capturing' | 'finalized' | 'partial';
  startedAt: string;
  finishedAt?: string | null;
  summary: TurnFileChangeSummaryVm;
  changes: TurnFileChangeVm[];
  limitationCodes: string[];
}

export interface CapturedTextSnapshotVm {
  version: FileVersionRefVm;
  content: string;
}

export interface FileComparisonVm {
  changeSetId: string;
  changeId: string;
  path: string;
  stats: { addedLines?: number | null; deletedLines?: number | null };
  before?: CapturedTextSnapshotVm | null;
  after?: CapturedTextSnapshotVm | null;
  limitationCode?: string | null;
}

export interface AcpElicitationRequestVm {
  elicitationId: string;
  message: string;
  toolCallId?: string | null;
  requestedSchema: Record<string, unknown>;
  raw: unknown;
}

// Navigation payload emitted after clicking "View details" in a system toast.
// It carries the complete attempt locator and a deduplication key.
export interface InterventionAttemptNavigateEventVm {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  dedupKey: string;
}

export interface ScheduledViewActionPayload {
  kind: 'completion' | 'failed' | 'attentionRequired' | 'missed';
  projectId: string;
  scheduledTaskId: string;
  occurrenceId?: string | null;
  taskId?: string | null;
  runId?: string | null;
  roundId?: string | null;
  attemptId?: string | null;
  dedupKey: string;
}

export type InterventionNavigateEventVm =
  | (InterventionAttemptNavigateEventVm & { targetType: 'conversation' })
  | (ScheduledViewActionPayload & { targetType: 'scheduled' });

export interface ScheduledNotificationEventVm {
  eventId: string;
  kind: 'completion' | 'failed' | 'attentionRequired' | 'missed';
  projectId: string;
  scheduledTaskId: string;
  occurrenceId?: string | null;
  errorCode?: string | null;
  errorParams?: Record<string, unknown> | null;
  links: {
    taskId?: string | null;
    runId?: string | null;
    roundId?: string | null;
    attemptId?: string | null;
  };
  missedCount?: number | null;
}

export interface ScheduledNativeNotificationInputVm extends ScheduledNotificationEventVm {
  title: string;
  body: string;
}

export interface ScheduledRuntimeSettingsVm {
  keepAwakeEnabled: boolean;
  keepAwakeEffective: boolean;
  completionNotificationsEnabled: boolean;
  enabledJobCount: number;
  occurrenceRetentionDays: number;
  powerErrorCode?: string | null;
}

export interface ScheduledRuntimeSettingsInputVm {
  keepAwakeEnabled: boolean;
  completionNotificationsEnabled: boolean;
  occurrenceRetentionDays: number;
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
  | { kind: 'scheduled-task-create' }
  | { kind: 'conversation-run'; projectId: string; taskId: string; runId: string; roundId?: string; attemptId?: string }
  | { kind: 'run-mode-management' }
  | { kind: 'agents' }
  | { kind: 'contexts' }
  | { kind: 'scheduled-tasks' }
  | { kind: 'scheduled-task-detail'; projectId: string; scheduledTaskId: string }
  | { kind: 'settings' };

export interface ScheduledTaskVm {
  id: string;
  projectId: string;
  workspaceName: string;
  title: string;
  enabled: boolean;
  mode: 'direct' | 'workflow' | 'auto' | string;
  sessionPolicy: 'new' | 'continuous' | string;
  schedule: ScheduledScheduleSpec;
  nextAt?: string | null;
  status: 'enabled' | 'paused' | 'completed' | 'failed' | string;
  lastTriggerAt?: string | null;
  lastTriggerStatus?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ScheduledOccurrenceVm {
  id: string;
  scheduledTaskId: string;
  scheduledAt: string;
  triggerKind: 'scheduled' | 'manual' | string;
  status: 'pending' | 'running' | 'retrying' | 'succeeded' | 'failed' | 'skipped' | 'missed' | 'attention_required' | string;
  attempt: number;
  errorCode?: string | null;
  errorParams?: Record<string, unknown> | null;
  taskId?: string | null;
  runId?: string | null;
  roundId?: string | null;
  attemptId?: string | null;
  startedAt?: string | null;
  finishedAt?: string | null;
}

export interface ScheduledTaskDiagnosticsVm {
  scheduledTaskId: string;
  projectId: string;
  nextAt?: string | null;
  lastStatus?: string | null;
  lastError?: string | null;
  runCount: number;
  retryCount: number;
  occurrences: ScheduledOccurrenceVm[];
}

export interface RunScheduledTaskResultVm {
  occurrence: ScheduledOccurrenceVm;
  taskId?: string | null;
  runId?: string | null;
  roundId?: string | null;
  attemptId?: string | null;
}

export type ScheduledEveryUnit = 'minutes' | 'hours';
export type ScheduledAtDisambiguation = 'earlier' | 'later';
export type ScheduledOverlapPolicy = 'skip_when_running' | 'retry_when_busy';
export type ScheduledSessionPolicy = 'new' | 'continuous';
export type ScheduledRepeatPreset =
  | 'Hourly'
  | 'Daily'
  | 'Weekdays'
  | { Weekly: { weekdays: string[] } };
export type ScheduledScheduleSpec =
  | { kind: 'At'; at: string; timezone: string }
  | { kind: 'Every'; every: { value: number; unit: ScheduledEveryUnit }; anchorAt: string; timezone: string }
  | { kind: 'Repeat'; preset: ScheduledRepeatPreset; hour: number; minute: number; timezone: string }
  | { kind: 'Cron'; expression: string; timezone: string };

export type ScheduledScheduleInput =
  | {
      kind: 'At';
      localDate: string;
      localTime: string;
      timezone: string;
      disambiguation: ScheduledAtDisambiguation;
    }
  | { kind: 'Every'; every: { value: number; unit: ScheduledEveryUnit }; anchorAt: string; timezone: string }
  | { kind: 'Repeat'; preset: ScheduledRepeatPreset; hour: number; minute: number; timezone: string }
  | { kind: 'Cron'; expression: string; timezone: string };

export interface CreateScheduledTaskInput extends ConversationCreateInput {
  schedule: ScheduledScheduleInput;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy?: ScheduledSessionPolicy;
}

export interface ScheduledTaskEditVm {
  scheduledTaskId: string;
  projectId: string;
  content: string;
  attachmentNames: string[];
  runMode: 'direct' | 'workflow' | 'auto' | string;
  workflowTemplateId?: string | null;
  includeInterview?: boolean | null;
  directConfig?: ConversationDirectConfigVm | null;
  autoConfig?: ConversationAutoConfigVm | null;
  schedule: ScheduledScheduleSpec;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
  directAgentType?: string | null;
  expectedUpdatedAt: string;
}

export interface UpdateScheduledTaskInput {
  scheduledTaskId: string;
  projectId: string;
  expectedUpdatedAt: string;
  content: string;
  runMode: string;
  workflowTemplateId?: string | null;
  includeInterview?: boolean | null;
  directConfig?: ConversationDirectConfigVm | null;
  autoConfig?: ConversationAutoConfigVm | null;
  attachmentPaths?: string[] | null;
  schedule: ScheduledScheduleInput;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
}

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
  scheduledTaskId?: string | null;
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
  mode: 'normal' | 'runtime-active' | 'stopping' | 'invalid-workflow' | 'runtime-error' | 'permission-blocked' | 'submitting' | string;
  submitTarget: 'acp-prompt' | 'queue-prompt' | 'permission-response' | 'none' | string;
  processingKind: 'sending' | 'launching' | 'processing' | 'thinking' | 'tool' | 'compacting' | 'responding' | 'stopping' | 'launching-next-node' | string;
  statusKey?: string | null;
  canStop: boolean;
  lockInput: boolean;
}

export interface AppExitRequestVm {
  requestId: string;
}

export type AppExitDecision = 'proceed' | 'cancel';

export interface ResolveAppExitInput {
  requestId: string;
  decision: AppExitDecision;
}

export interface ConversationQueuedPromptVm {
  id: string;
  content: string;
  attachmentCount: number;
  createdAt: string;
}

export interface ConversationPromptQueueVm {
  revision: number;
  items: ConversationQueuedPromptVm[];
  maxItems: number;
}

export interface ConversationAttemptLifecycleVm {
  runtime: ConversationRuntimeFacetVm;
  acp: ConversationAcpFacetVm;
  displayStatus: string;
  runtimeDisplay: RuntimeDisplayVm;
  continueKind?: 'action' | null;
  composer: ConversationComposerVm;
  promptQueue?: ConversationPromptQueueVm | null;
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
  sessionEstablished?: boolean;
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
  inputAttachments: AssetItemVm[];
  workflowStatus: string;
  workflowValid: boolean;
  workflowError?: WorkflowErrorVm | null;
  workflowJson?: string | null;
  workflowGraph: GraphVm;
  resumable: boolean;
  pauseReason?: string | null;
  runtimeErrorMessage?: string | null;
  scheduledTaskId?: string | null;
}

export interface ConversationSessionSwitchVm {
  selectedSession?: AcpSessionVm | null;
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
  sessionEstablished?: boolean;
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
  bootstrapConfigOptions?: Record<string, string>;
  acceptanceModelId?: string | null;
  acceptanceConfigOptions?: Record<string, string>;
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

export interface AppExitPreparationWarningVm {
  code: string;
  params: Record<string, unknown>;
}

export interface AppExitPreparationVm {
  warnings: AppExitPreparationWarningVm[];
}
