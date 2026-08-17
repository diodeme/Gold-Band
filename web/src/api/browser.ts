import type { AcpRawFramePageVm, AcpRawFrameQueryInput, AcpSessionQueryInput, AcpSessionVm, AgentRegistryVm, AppearancePreference, AppBootstrapVm, AutoTemplate, ContentVm, ConversationAutoConfigVm, ConversationCreateInput, ConversationRunModeVm, ConversationRunVm, ConversationSearchResultVm, ConversationSidebarVm, ConversationValidationResultVm, ConversationWorkspaceVm, CreateTaskInput, DesktopLanguage, FileRevisionVm, GitStateChangedEventVm, LocalClaudeStatusVm, LogPageVm, LogQueryInput, ManagedAgentInput, PersonalizationPreference, PreferencesVm, ProfileInput, ProfileVm, RoundDetailVm, RoundSelection, RunDetailVm, RunSummaryVm, RunScheduledTaskResultVm, ScheduledOccurrenceVm, ScheduledTaskDiagnosticsVm, ScheduledTaskEditVm, ScheduledTaskVm, TaskDetailVm, TaskListVm, UpdateBadgeStateVm, UpdateScheduledTaskInput, UpdateStatusVm, UpdaterSettingsVm, WorkflowDsl, WorkflowTemplateStore, WorkflowVm, WorkspaceFileChangedEventVm } from '../types';
import { mockAgentRegistry, mockBootstrap, mockContent, mockErrorBlockedConversationRun, mockErrorBlockedConversationSession, mockLogPage, mockRoundDetail, mockRunDetail, mockTaskDetail, mockTaskList, mockWorkflow, mockWorkflowTemplates } from '../mockData';
import type { RuntimeApi, ScheduledOccurrenceUpdatedEventVm, ScheduledTaskUpdatedEventVm } from './client';
import type { GitCommitVm, GitHubOperationVm, GitOperationVm } from '../types';
import { browserPreviewState } from './browserState';
import { localTimestamp, toRoundSelectionInput } from './shared';
import { scheduledScheduleSpecFromInput } from '@/lib/scheduled-task-authoring';
import { normalizeFontCatalogFamilies } from '@/lib/font-families';
import { boundedRecentWallpapers } from '@/lib/wallpaper';

const browserFontCandidates = [
  'MiSans', 'Maple Mono NF CN', 'Microsoft YaHei UI', 'Microsoft YaHei', 'DengXian', 'DengXian Light', 'SimHei', 'SimSun', 'NSimSun', 'KaiTi', 'FangSong', 'YouYuan', 'LiSu', 'STXihei', 'STSong', 'STKaiti', 'STFangsong', 'PingFang SC', 'PingFang TC', 'PingFang HK', 'Hiragino Sans GB', 'Songti SC', 'Kaiti SC', 'Heiti SC', 'Heiti TC', 'Noto Sans CJK SC', 'Noto Sans CJK TC', 'Noto Sans SC', 'Noto Serif SC', 'Source Han Sans SC', 'Source Han Serif SC', 'Sarasa Gothic SC', 'LXGW WenKai', 'MiSans', 'HarmonyOS Sans SC', 'WenQuanYi Micro Hei', 'WenQuanYi Zen Hei', 'Segoe UI', 'Segoe UI Variable', 'Yu Gothic UI', 'Meiryo', 'Malgun Gothic', 'SF Pro Text', 'SF Pro Display', 'Inter', 'Roboto', 'Arial', 'Helvetica Neue', 'Helvetica', 'Ubuntu', 'Cantarell', 'DejaVu Sans', 'Liberation Sans',
] as const;

type LocalFontData = { family: string };
type LocalFontWindow = Window & { queryLocalFonts?: () => Promise<LocalFontData[]> };

const browserConversationRuns = new Map<string, ConversationRunVm>();
const browserScheduledTasks: ScheduledTaskVm[] = [];
const browserScheduledTaskDefinitions = new Map<string, ScheduledTaskEditVm>();
const browserScheduledTaskListeners = new Set<(event: ScheduledTaskUpdatedEventVm) => void>();
const browserScheduledOccurrences = new Map<string, ScheduledOccurrenceVm[]>();
const browserScheduledOccurrenceListeners = new Set<(event: ScheduledOccurrenceUpdatedEventVm) => void>();
let browserScheduledTaskSequence = 0;
let browserScheduledRuntimeSettings = {
  keepAwakeEnabled: false,
  keepAwakeEffective: false,
  completionNotificationsEnabled: true,
  enabledJobCount: 0,
  occurrenceRetentionDays: 30,
  powerErrorCode: null,
};

function emitBrowserScheduledTaskUpdated(task: ScheduledTaskVm) {
  const event: ScheduledTaskUpdatedEventVm = {
    projectId: task.projectId,
    scheduledTaskId: task.id,
    status: task.status,
  };
  browserScheduledTaskListeners.forEach((listener) => listener(event));
}

function emitBrowserScheduledOccurrenceUpdated(occurrence: ScheduledOccurrenceVm, projectId: string) {
  const event: ScheduledOccurrenceUpdatedEventVm = {
    projectId,
    scheduledTaskId: occurrence.scheduledTaskId,
    occurrenceId: occurrence.id,
    status: occurrence.status,
    errorCode: occurrence.errorCode ?? null,
    taskId: occurrence.taskId ?? null,
    runId: occurrence.runId ?? null,
  };
  browserScheduledOccurrenceListeners.forEach((listener) => listener(event));
}
const browserGitOperations = new Map<string, GitOperationVm>();
const browserGitOperationListeners = new Set<(operation: GitOperationVm) => void>();
const browserGitStateListeners = new Set<(event: GitStateChangedEventVm) => void>();
const browserGitFailurePreviewRemote = 'fork';
const browserGitMutationPreviewDelayMs = 700;
const browserGitHubReadPreviewDelayMs = 700;
const browserGitStagePreviewPath = 'web/src/components/workspace/SourceControlWorkspacePanel.tsx';
let browserGitStagePreviewApplied = false;
const browserGitHubOperations = new Map<string, GitHubOperationVm>();
const browserGitHubOperationListeners = new Set<(operation: GitHubOperationVm) => void>();

const browserGitCommits: GitCommitVm[] = [
  {
    oid: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241',
    parentOids: ['8dc4ac2a3fc32f88e2348c0ea6682907c38acc89'],
    subject: 'feat(git): add source control foundation',
    body: 'Add the typed source control service and right workspace UI.',
    author: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-10T12:00:00Z' },
    committer: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-10T12:00:00Z' },
    refs: [{ fullName: 'refs/heads/feature/source-control', shortName: 'feature/source-control', kind: 'local-branch' }],
    sourceRef: 'refs/heads/feature/source-control',
    runtimeCheckpoint: false,
  },
  {
    oid: '8dc4ac2a3fc32f88e2348c0ea6682907c38acc89',
    parentOids: ['73cc8bb94b23de03f11b90918c80f44db3299502'],
    subject: 'refactor: prepare workspace resources',
    body: '',
    author: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-09T10:00:00Z' },
    committer: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-09T10:00:00Z' },
    refs: [{ fullName: 'refs/heads/main', shortName: 'main', kind: 'local-branch' }],
    sourceRef: 'refs/heads/main',
    runtimeCheckpoint: false,
  },
  {
    oid: '73cc8bb94b23de03f11b90918c80f44db3299502',
    parentOids: [],
    subject: 'chore: initialize repository',
    body: '',
    author: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-08T08:00:00Z' },
    committer: { name: 'Gold Band', email: 'dev@example.com', timestamp: '2026-08-08T08:00:00Z' },
    refs: [],
    sourceRef: 'refs/heads/main',
    runtimeCheckpoint: false,
  },
];
const browserGitOlderCommitCount = 300;
const browserGitHistoryCommits: GitCommitVm[] = [
  ...browserGitCommits,
  ...Array.from({ length: browserGitOlderCommitCount }, (_, index) => {
    const oid = browserGitPreviewOid(index);
    const parentOid = index + 1 < browserGitOlderCommitCount
      ? browserGitPreviewOid(index + 1)
      : null;
    const timestamp = new Date(Date.UTC(2026, 7, 7) - index * 60_000).toISOString();
    return {
      oid,
      parentOids: parentOid ? [parentOid] : [],
      subject: `preview: historical commit ${index + 1}`,
      body: '',
      author: { name: 'Gold Band', email: 'dev@example.com', timestamp },
      committer: { name: 'Gold Band', email: 'dev@example.com', timestamp },
      refs: [],
      sourceRef: 'refs/heads/main',
      runtimeCheckpoint: false,
    };
  }),
];

function browserGitPreviewOid(index: number) {
  return (index + 1_000).toString(16).padStart(40, '0');
}

function browserGitCommit(oid: string) {
  return browserGitHistoryCommits.find((candidate) => candidate.oid === oid) ?? browserGitCommits[0];
}

function browserAgentIdentity(agentType: string) {
  const agent = mockAgentRegistry.agents.find((candidate) => candidate.agentType === agentType);
  return agent ? {
    agentType: agent.agentType,
    displayName: agent.displayName,
    iconKey: agent.iconKey,
  } : null;
}

function browserCompletedConversationRun(): ConversationRunVm {
  const run = structuredClone(mockErrorBlockedConversationRun);
  run.runId = 'run-052';
  run.title = '斜杠命令预览';
  run.runMode = 'direct';
  run.directConfig = { agentType: 'claude-acp' };
  run.agentIdentity = browserAgentIdentity('claude-acp');
  run.runStatus = 'completed';
  run.runOutcome = 'success';
  run.pauseReason = null;
  run.runtimeErrorMessage = null;
  run.selectedSession = {
    ...mockErrorBlockedConversationSession,
    sessionId: 'browser-session-052',
    roundId: 'round-001',
    nodeId: 'dev',
    attemptId: 'attempt-001',
    providerCwd: 'D:/Projects/code/ai/Gold-Band',
    cwd: 'D:/Projects/code/ai/Gold-Band',
    status: 'completed',
    stopReason: 'end_turn',
    systemPromptAppend: [
      '# Browser system prompt',
      '',
      'This **system prompt** verifies the rendered/source workspace modes.',
      '',
      '- Attempt: `attempt-001`',
      '- Workspace: `D:/Projects/code/ai/Gold-Band`',
    ].join('\n'),
    usage: {
      used: 25_400,
      size: 258_400,
      inputTokens: 18_760,
      outputTokens: 2_140,
      cachedReadTokens: 4_200,
      cachedWriteTokens: 300,
      totalTokens: 25_400,
    },
    events: [
      {
        id: 'browser-user-prompt-052',
        seq: 1,
        timestamp: '2026-08-04 10:00',
        kind: 'userTextDelta',
        content: [
          '<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">',
          '# Stable system prompt',
          '',
          'You are the Gold Band browser preview agent.',
          '</hidden>',
          '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">',
          '# Runtime context',
          '',
          '- Attempt: `attempt-001`',
          '- Workspace: `D:/Projects/code/ai/Gold-Band`',
          '</hidden>',
          '> 这是用户自己输入的 Markdown 引用。',
          '',
          '请更新工作区配置并补充说明。',
        ].join('\n'),
        raw: {
          promptId: 'browser-prompt-052',
          quotes: Array.from({ length: 8 }, (_, index) => ({
            id: `browser-quote-052-${index + 1}`,
            sourceMessageKey: `textDelta-browser-agent-message-${index + 1}`,
            text: index === 0
              ? `请优先检查工作区配置中的权限边界。\n${'这是一段用于验证长引用内部换行与滚动边界的内容。'.repeat(16)}`
              : `第 ${index + 1} 条引用：补充核对配置项、权限范围和对应说明。`,
          })),
          attachments: [{
            name: 'browser-zoom-fixture.png',
            path: 'task-inputs/browser-zoom-fixture.png',
            type: 'image/png',
            size: 68,
          }],
        },
      },
      {
        id: 'browser-tool-call-052',
        seq: 2,
        timestamp: '2026-08-04 10:01',
        kind: 'toolCall',
        title: '更新工作区文件',
        toolCallId: 'browser-tool-052',
        status: 'completed',
        raw: { toolCallId: 'browser-tool-052', title: '更新工作区文件', status: 'completed' },
      },
      {
        id: 'browser-context-compaction-052',
        seq: 3,
        timestamp: '2026-08-04 10:01',
        startedAt: '2026-08-04 10:01:00',
        endedAt: '2026-08-04 10:01:02',
        kind: 'contextCompaction',
        status: 'completed',
        raw: {
          contextCompaction: {
            usageBefore: { used: 128_000, size: 258_400 },
          },
        },
      },
      {
        id: 'browser-file-change-set-052',
        seq: 4,
        timestamp: '2026-08-04 10:01',
        kind: 'fileChangeSet',
        status: 'finalized',
        raw: {
          changeSetId: browserTurnFileChangeSet.id,
          summary: browserTurnFileChangeSet.summary,
        },
      },
      {
        id: 'browser-agent-message-052',
        seq: 5,
        timestamp: '2026-08-04 10:01',
        kind: 'textDelta',
        content: '配置与说明已更新。',
        status: 'completed',
        raw: {},
      },
    ],
    eventPage: {
      loadedCount: 5,
      total: 5,
      oldestSeq: 1,
      newestSeq: 5,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    config: {
      modelOverrideId: null,
      permissionModeOverrideId: null,
      currentModelId: 'default',
      currentModelName: 'Default (recommended)',
      currentModeId: 'bypassPermissions',
      currentModeName: 'Bypass Permissions',
      configOptions: [
        {
          id: 'model',
          category: 'model',
          options: [
            { value: 'default', name: 'Default (recommended)' },
            { value: 'glm-5.2-hs', name: 'GLM 5.2' },
          ],
        },
        {
          id: 'mode',
          category: 'mode',
          options: [
            { value: 'bypassPermissions', name: 'Bypass Permissions' },
          ],
        },
        {
          id: 'effort',
          category: 'thought_level',
          type: 'select',
          currentValue: 'default',
          options: [
            { value: 'default', name: 'Default' },
            { value: 'low', name: 'Low' },
            { value: 'high', name: 'High' },
            { value: 'max', name: 'Max' },
          ],
        },
      ],
    },
  };
  const attempt = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0];
  if (attempt?.lifecycle) {
    attempt.status = 'completed';
    attempt.outcome = 'success';
    attempt.lifecycle = {
      ...attempt.lifecycle,
      runtime: {
        ...attempt.lifecycle.runtime,
        status: 'completed',
        outcome: 'success',
        pauseReason: null,
        active: false,
        continuable: false,
        phase: 'completed',
      },
      acp: {
        ...attempt.lifecycle.acp,
        liveTurnActivity: 'idle',
        latestTurnStatus: 'completed',
        stopping: false,
      },
      displayStatus: 'success',
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'responding',
        statusKey: null,
        canStop: false,
        lockInput: false,
      },
    };

  }
  return run;
}

const browserQueuedPromptDrafts = [
  {
    id: 'browser-queued-1',
    content: '完成当前修改后，补充对应的回归测试。',
    quotes: [],
    attachmentPaths: ['C:/browser/mock.png'],
    createdAt: '2026-08-07T08:00:00Z',
  },
  {
    id: 'browser-queued-2',
    content: '检查深色主题下的输入区层级。',
    quotes: [
      { id: 'browser-quote-1', sourceMessageKey: 'browser-message-1', text: '第一段引用' },
      { id: 'browser-quote-2', sourceMessageKey: 'browser-message-2', text: '第二段引用' },
    ],
    attachmentPaths: [],
    createdAt: '2026-08-07T08:00:01Z',
  },
  {
    id: 'browser-queued-3',
    content: '把关键设计决策同步到产品文档。',
    quotes: [],
    attachmentPaths: [],
    createdAt: '2026-08-07T08:00:02Z',
  },
  {
    id: 'browser-queued-4',
    content: '验证停止后队列仍然可编辑和删除。',
    quotes: [],
    attachmentPaths: [],
    createdAt: '2026-08-07T08:00:03Z',
  },
  {
    id: 'browser-queued-5',
    content: '最后整理本轮变更摘要。',
    quotes: [],
    attachmentPaths: [],
    createdAt: '2026-08-07T08:00:04Z',
  },
];

function browserQueuedConversationRun(): ConversationRunVm {
  const run = browserCompletedConversationRun();
  run.runId = 'run-053';
  run.title = 'Direct 待发送队列预览';
  run.runStatus = 'running';
  run.runOutcome = null;
  if (run.selectedSession) {
    run.selectedSession = {
      ...run.selectedSession,
      sessionId: 'browser-session-053',
      status: 'running',
      stopReason: null,
    };
  }
  const attempt = run.sessionTree.rounds[0]?.nodes[0]?.attempts[0];
  if (attempt?.lifecycle) {
    attempt.status = 'running';
    attempt.outcome = null;
    attempt.lifecycle = {
      ...attempt.lifecycle,
      runtime: {
        ...attempt.lifecycle.runtime,
        status: 'running',
        outcome: null,
        active: true,
        current: true,
        continuable: false,
        phase: 'provider-running',
      },
      acp: {
        ...attempt.lifecycle.acp,
        liveTurnActivity: 'running',
        latestTurnStatus: 'none',
        stopping: false,
      },
      displayStatus: 'running',
      composer: {
        mode: 'runtime-active',
        submitTarget: 'queue-prompt',
        processingKind: 'responding',
        statusKey: null,
        canStop: true,
        lockInput: false,
      },
      promptQueue: {
        revision: 5,
        maxItems: 10,
        items: browserQueuedPromptDrafts.map((item) => ({
          id: item.id,
          content: item.content,
          attachmentCount: item.attachmentPaths.length,
          quoteCount: item.quotes.length,
          createdAt: item.createdAt,
        })),
      },
    };
  }
  return run;
}

const browserTurnFileChangeSet = {
  id: 'browser-change-set-052',
  turnId: 'browser-turn-052',
  promptEventId: 'browser-prompt-052',
  branchId: 'root',
  status: 'finalized' as const,
  startedAt: '2026-08-04 10:00',
  finishedAt: '2026-08-04 10:01',
  summary: {
    fileCount: 3,
    addedFiles: 1,
    modifiedFiles: 1,
    deletedFiles: 1,
    addedLines: 8,
    deletedLines: 3,
  },
  changes: [
    {
      id: 'browser-added-readme',
      changeKind: 'added' as const,
      logicalPath: 'docs/workspace-notes.md',
      text: true,
      addedLines: 4,
      deletedLines: 0,
    },
    {
      id: 'browser-modified-config',
      changeKind: 'modified' as const,
      logicalPath: 'src/config.json',
      text: true,
      addedLines: 4,
      deletedLines: 2,
    },
    {
      id: 'browser-deleted-legacy',
      changeKind: 'deleted' as const,
      logicalPath: 'src/legacy-config.json',
      text: true,
      addedLines: 0,
      deletedLines: 1,
    },
  ],
  limitationCodes: [],
};

const browserWorkspaceRoot = '/default';
const browserWorkspaceFiles = new Map<string, string>([
  ['/default/README.md', '# Gold Band\n\n右侧工作区文件预览。\n'],
  ['/default/src/main.rs', 'fn main() {\n    println!("Gold Band");\n}\n'],
  ['/default/src/config.json', '{\n  "workspace": "default"\n}\n'],
  ['/default/assets/logo.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="120"><rect width="240" height="120" rx="24" fill="#b9922e"/><text x="120" y="70" text-anchor="middle" fill="#18140a" font-size="24">Gold Band</text></svg>'],
]);
const browserFileRevisions = new Map<string, number>();
const browserWorkspaceFileListeners = new Set<(event: WorkspaceFileChangedEventVm) => void>();
const browserExternalFileGrants = new Map<string, { canonicalPath: string; expiresAtMs: number }>();
let browserExternalGrantRevision = 0;

function issueBrowserExternalFileGrant(canonicalPath: string) {
  browserExternalGrantRevision += 1;
  const token = `browser-external:${browserExternalGrantRevision}:${canonicalPath}`;
  const expiresAtMs = Date.now() + 30 * 60_000;
  browserExternalFileGrants.set(token, { canonicalPath, expiresAtMs });
  return {
    token,
    permissions: ['read', 'write'] as Array<'read' | 'write'>,
    expiresAtMs: String(expiresAtMs),
  };
}

function browserExternalGrantValid(token: string | null | undefined, canonicalPath: string) {
  if (!token) return false;
  const grant = browserExternalFileGrants.get(token);
  return Boolean(grant && grant.canonicalPath === canonicalPath && grant.expiresAtMs > Date.now());
}

function browserFileRevision(path: string, content: string): FileRevisionVm {
  const revision = browserFileRevisions.get(path) ?? 0;
  return {
    byteLength: new TextEncoder().encode(content).byteLength,
    modifiedAtNs: String(revision),
    contentHash: `browser-${revision}-${content.length}`,
  };
}

function browserRelativePath(path: string) {
  return path.startsWith(`${browserWorkspaceRoot}/`) ? path.slice(browserWorkspaceRoot.length + 1) : null;
}

function browserDirectoryEntries(relativePath: string) {
  const directory = relativePath ? `${browserWorkspaceRoot}/${relativePath}` : browserWorkspaceRoot;
  const prefix = `${directory}/`;
  const seen = new Set<string>();
  const entries: import('../types').WorkspaceDirectoryEntryVm[] = [];
  for (const [path, content] of browserWorkspaceFiles) {
    if (!path.startsWith(prefix)) continue;
    const remainder = path.slice(prefix.length);
    const [name, ...rest] = remainder.split('/');
    if (seen.has(name)) continue;
    seen.add(name);
    const childPath = `${prefix}${name}`;
    const childRelativePath = browserRelativePath(childPath) ?? name;
    const directoryEntry = rest.length > 0;
    entries.push({
      name,
      relativePath: childRelativePath,
      canonicalPath: childPath,
      kind: directoryEntry ? 'directory' : 'file',
      hasChildren: directoryEntry,
      byteLength: directoryEntry ? null : new TextEncoder().encode(content).byteLength,
      modifiedAtNs: directoryEntry ? null : String(browserFileRevisions.get(path) ?? 0),
    });
  }
  return entries.sort((left, right) => Number(left.kind !== 'directory') - Number(right.kind !== 'directory') || left.name.localeCompare(right.name));
}

function browserSvgDataUrl(content: string) {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(content)}`;
}

export const browserApi: RuntimeApi = {
  async subscribeScheduledNotifications() {
    return () => {};
  },
  async sendScheduledNativeNotification() {},
  async getScheduledRuntimeSettings() {
    return structuredClone(browserScheduledRuntimeSettings);
  },
  async saveScheduledRuntimeSettings(input) {
    if (input.occurrenceRetentionDays < 1 || input.occurrenceRetentionDays > 3650) {
      throw { code: 'SCHEDULED_VALIDATION_FAILED', params: { field: 'occurrenceRetentionDays', minimum: 1, maximum: 3650, actual: input.occurrenceRetentionDays } };
    }
    browserScheduledRuntimeSettings = {
      ...browserScheduledRuntimeSettings,
      ...input,
      keepAwakeEffective: false,
    };
    return structuredClone(browserScheduledRuntimeSettings);
  },
  async subscribeScheduledTaskUpdates(listener) {
    browserScheduledTaskListeners.add(listener);
    return () => browserScheduledTaskListeners.delete(listener);
  },
  async subscribeScheduledOccurrenceUpdates(listener) {
    browserScheduledOccurrenceListeners.add(listener);
    return () => browserScheduledOccurrenceListeners.delete(listener);
  },
  getGitCapability() {
    return Promise.resolve({ status: 'repository-required', repoRoot: null, commonDir: null, head: null });
  },
  initializeGitRepository() {
    return Promise.resolve({ status: 'head-required', repoRoot: null, commonDir: null, head: null });
  },
  getSourceControlSnapshot(projectId, workspacePath) {
    const resolvedWorkspacePath = workspacePath ?? '/preview/gold-band';
    return Promise.resolve({
      repository: {
        projectId,
        repoRoot: '/preview/gold-band',
        commonDir: '/preview/gold-band/.git',
        workspacePath: resolvedWorkspacePath,
        headOid: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241',
        currentBranch: 'feature/source-control',
        detached: false,
        unborn: false,
        upstream: { name: 'origin/feature/source-control', ahead: 1, behind: 0 },
        remotes: [
          { name: 'origin', fetchUrls: ['https://github.com/example/gold-band.git'], pushUrls: ['https://github.com/example/gold-band.git'] },
          { name: browserGitFailurePreviewRemote, fetchUrls: ['https://github.com/example/gold-band-fork.git'], pushUrls: ['https://github.com/example/gold-band-fork.git'] },
        ],
        lock: { locked: false, owner: null, operation: null },
        revision: 'browser-preview-revision',
      },
      status: {
        snapshotRevision: 'browser-preview-revision',
        branch: { oid: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241', head: 'feature/source-control', upstream: 'origin/feature/source-control', ahead: 1, behind: 0 },
        conflicts: [],
        staged: [
          { path: 'src/git/source_control.rs', oldPath: null, kind: 'added', indexStatus: 'A', worktreeStatus: null, binary: false, submodule: false, addedLines: 420, deletedLines: 0 },
          ...(browserGitStagePreviewApplied ? [{ path: browserGitStagePreviewPath, oldPath: null, kind: 'modified' as const, indexStatus: 'M', worktreeStatus: null, binary: false, submodule: false, addedLines: 34, deletedLines: 8 }] : []),
        ],
        unstaged: browserGitStagePreviewApplied ? [] : [{ path: browserGitStagePreviewPath, oldPath: null, kind: 'modified', indexStatus: null, worktreeStatus: 'M', binary: false, submodule: false, addedLines: 34, deletedLines: 8 }],
        untracked: [{ path: 'docs/source-control-notes.md', oldPath: null, kind: 'untracked', indexStatus: null, worktreeStatus: '?', binary: false, submodule: false, addedLines: null, deletedLines: null }],
        operationInProgress: null,
      },
      refs: [
        { fullName: 'refs/heads/feature/source-control', shortName: 'feature/source-control', kind: 'local-branch', targetOid: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241', peeledOid: null, upstream: 'origin/feature/source-control', ahead: 1, behind: 0, checkedOutWorktreePaths: [resolvedWorkspacePath] },
        { fullName: 'refs/heads/main', shortName: 'main', kind: 'local-branch', targetOid: '8dc4ac2a3fc32f88e2348c0ea6682907c38acc89', peeledOid: null, upstream: 'origin/main', ahead: 0, behind: 0, checkedOutWorktreePaths: [] },
      ],
      worktrees: [{ path: resolvedWorkspacePath, headOid: '9e1d4f31c17c9bb7f382e130e8db2ab98cf58241', branch: 'refs/heads/feature/source-control', main: workspacePath == null, detached: false, locked: false, lockReason: null, prunable: false, ownership: 'user', runtimeStatus: null }],
      stashes: [],
    });
  },
  getGitHistory(_projectId, _workspacePath, query) {
    const cursorMatch = query.cursor?.match(/^browser-history:(\d+)$/);
    const offset = cursorMatch ? Number(cursorMatch[1]) : 0;
    const limit = query.limit ?? 300;
    const nextOffset = Math.min(offset + limit, browserGitHistoryCommits.length);
    return Promise.resolve({
      commits: structuredClone(browserGitHistoryCommits.slice(offset, nextOffset)),
      nextCursor: nextOffset < browserGitHistoryCommits.length
        ? `browser-history:${nextOffset}`
        : null,
      revision: 'browser-preview-revision',
    });
  },
  getGitCommitDetail(_projectId, _workspacePath, oid) {
    const commit = browserGitCommit(oid);
    return Promise.resolve({
      commit: structuredClone(commit),
      files: [{
        path: commit.oid === browserGitCommits[0].oid ? 'src/git/source_control.rs' : 'README.md',
        oldPath: null,
        kind: 'modified',
        binary: false,
        addedLines: commit.oid === browserGitCommits[0].oid ? 420 : 12,
        deletedLines: commit.oid === browserGitCommits[0].oid ? 8 : 2,
      }],
    });
  },
  getGitCommitReview(_projectId, _workspacePath, query) {
    const entries = query.selectedOids.map((oid) => {
      const commit = browserGitCommit(oid);
      return {
        beforeOid: commit.parentOids[0] ?? null,
        beforePath: commit.oid === browserGitCommits[0].oid ? 'src/git/source_control.rs' : 'README.md',
        afterOid: commit.oid,
        path: commit.oid === browserGitCommits[0].oid ? 'src/git/source_control.rs' : 'README.md',
      };
    });
    const files = Array.from(new Map(entries.map((entry) => [entry.path, {
      ...entry,
      oldPath: null,
      kind: 'modified' as const,
      binary: false,
      addedLines: 24,
      deletedLines: 6,
    }])).values());
    return Promise.resolve({
      selectedOids: [...query.selectedOids],
      revision: 'browser-preview-revision',
      files,
      totals: {
        commitCount: query.selectedOids.length,
        fileCount: files.length,
      },
    });
  },
  getGitCommitReachability(_projectId, _workspacePath, query) {
    const commit = browserGitCommits.find((candidate) => candidate.oid === query.oid) ?? browserGitCommits[0];
    return Promise.resolve({
      oid: commit.oid,
      containingRefs: structuredClone(commit.refs),
      targetRef: query.targetRef,
      targetOid: browserGitCommits[0].oid,
      targetPath: commit.oid === browserGitCommits[0].oid ? 'tip' as const : 'direct' as const,
      firstMergeOid: null,
      parentOids: [...commit.parentOids],
    });
  },
  async executeGitMutation(projectId, workspacePath, input) {
    await new Promise((resolve) => setTimeout(resolve, browserGitMutationPreviewDelayMs));
    if (input.kind === 'stage-paths' && input.paths.includes(browserGitStagePreviewPath)) {
      browserGitStagePreviewApplied = true;
    }
    if (input.kind === 'unstage-paths' && input.paths.includes(browserGitStagePreviewPath)) {
      browserGitStagePreviewApplied = false;
    }
    if (['stage-paths', 'stage-all', 'unstage-paths', 'unstage-all'].includes(input.kind)) {
      const snapshot = await browserApi.getSourceControlSnapshot(projectId, workspacePath);
      return {
        scope: 'workspace',
        status: snapshot.status,
        repositoryRevision: snapshot.repository.revision,
      };
    }
    return { scope: 'repository' };
  },
  async getGitComparison(_projectId, source) {
    const staged = source.kind === 'workspace' && source.area === 'staged';
    const pullRequest = source.kind === 'github-pr';
    if (pullRequest) await new Promise((resolve) => setTimeout(resolve, browserGitHubReadPreviewDelayMs));
    return {
      path: source.path,
      stats: { addedLines: staged ? 3 : pullRequest ? 4 : 2, deletedLines: staged ? 0 : 1 },
      before: { content: 'export const sourceControl = false;\n' },
      after: { content: pullRequest
        ? 'export const sourceControl = true;\nexport const gitHubPullRequests = true;\n'
        : 'export const sourceControl = true;\nexport const gitHub = true;\n' },
      limitationCode: null,
    };
  },
  startGitOperation(_projectId, workspacePath, input) {
    const failurePreview = input.kind === 'push' && input.remote === browserGitFailurePreviewRemote;
    const operationId = `browser-git-${Date.now().toString(36)}`;
    const queued: GitOperationVm = {
      operationId,
      kind: input.kind,
      repositoryCommonDir: '/preview/gold-band/.git',
      workspacePath,
      status: 'queued',
      cancelable: true,
      startedAt: null,
      completedAt: null,
      error: null,
    };
    const terminal: GitOperationVm = {
      operationId,
      kind: input.kind,
      repositoryCommonDir: '/preview/gold-band/.git',
      workspacePath,
      status: failurePreview ? 'failed' : 'succeeded',
      cancelable: false,
      startedAt: new Date().toISOString(),
      completedAt: new Date().toISOString(),
      error: failurePreview ? {
        code: 'git.authentication-failed',
        params: {
          exitCode: 128,
          reason: "fatal: Authentication failed for 'https://github.com/example/gold-band-fork.git/'\nVerify the account used by the credential helper has write access to this repository.",
        },
      } : null,
    };
    browserGitOperations.set(operationId, queued);
    setTimeout(() => {
      const current = browserGitOperations.get(operationId);
      if (!current || current.status === 'cancelled') return;
      browserGitOperations.set(operationId, terminal);
      for (const listener of browserGitOperationListeners) listener(terminal);
    }, browserGitMutationPreviewDelayMs);
    return Promise.resolve(queued);
  },
  getGitOperation(operationId) {
    const operation = browserGitOperations.get(operationId);
    return operation
      ? Promise.resolve(operation)
      : Promise.reject({ code: 'git.operation-not-found', params: { operationId } });
  },
  cancelGitOperation(operationId) {
    const operation = browserGitOperations.get(operationId);
    if (!operation) return Promise.reject({ code: 'git.operation-not-found', params: { operationId } });
    const cancelled = { ...operation, status: 'cancelled' as const, cancelable: false, completedAt: new Date().toISOString() };
    browserGitOperations.set(operationId, cancelled);
    queueMicrotask(() => {
      for (const listener of browserGitOperationListeners) listener(cancelled);
    });
    return Promise.resolve(cancelled);
  },
  startGitStateMonitor(_projectId, _workspacePath) {
    return Promise.resolve();
  },
  stopGitStateMonitor(_projectId, _workspacePath) {
    return Promise.resolve();
  },
  subscribeGitOperationUpdates(listener) {
    browserGitOperationListeners.add(listener);
    return Promise.resolve(() => browserGitOperationListeners.delete(listener));
  },
  subscribeGitStateChanges(listener) {
    browserGitStateListeners.add(listener);
    return Promise.resolve(() => browserGitStateListeners.delete(listener));
  },
  async getGitHubCapability() {
    await new Promise((resolve) => setTimeout(resolve, browserGitHubReadPreviewDelayMs));
    return { status: 'ready' as const, version: 'gh version 2.79.0', host: 'github.com', account: 'gold-band-preview', repository: 'example/gold-band', remote: 'origin', defaultBranch: 'main' };
  },
  startGitHubLogin(_projectId, _workspacePath, host) {
    const operation: GitHubOperationVm = { operationId: `browser-gh-login-${Date.now().toString(36)}`, kind: 'login', host, status: 'succeeded', cancelable: false, startedAt: new Date().toISOString(), completedAt: new Date().toISOString(), error: null, resultUrl: null };
    browserGitHubOperations.set(operation.operationId, operation);
    queueMicrotask(() => {
      for (const listener of browserGitHubOperationListeners) listener(operation);
    });
    return Promise.resolve(operation);
  },
  getGitHubOperation(operationId) {
    const operation = browserGitHubOperations.get(operationId);
    return operation ? Promise.resolve(operation) : Promise.reject({ code: 'github.operation-not-found', params: { operationId } });
  },
  cancelGitHubOperation(operationId) {
    const operation = browserGitHubOperations.get(operationId);
    if (!operation) return Promise.reject({ code: 'github.operation-not-found', params: { operationId } });
    const cancelled = { ...operation, status: 'cancelled' as const, cancelable: false, completedAt: new Date().toISOString() };
    browserGitHubOperations.set(operationId, cancelled);
    queueMicrotask(() => {
      for (const listener of browserGitHubOperationListeners) listener(cancelled);
    });
    return Promise.resolve(cancelled);
  },
  subscribeGitHubOperationUpdates(listener) {
    browserGitHubOperationListeners.add(listener);
    return Promise.resolve(() => browserGitHubOperationListeners.delete(listener));
  },
  preflightGitHubPullRequest(_projectId, _workspacePath, input) {
    return Promise.resolve({ remote: 'origin', head: input.head, base: input.base, aheadBy: 3, headPublished: true, existingPullRequest: null });
  },
  startGitHubPullRequestCreate(_projectId, _workspacePath, input) {
    const operation: GitHubOperationVm = {
      operationId: `browser-gh-pr-${Date.now().toString(36)}`,
      kind: 'pr-create',
      host: input.host,
      status: 'succeeded',
      cancelable: false,
      startedAt: new Date().toISOString(),
      completedAt: new Date().toISOString(),
      error: null,
      resultUrl: 'https://github.com/example/gold-band/pull/43',
    };
    browserGitHubOperations.set(operation.operationId, operation);
    queueMicrotask(() => {
      for (const listener of browserGitHubOperationListeners) listener(operation);
    });
    return Promise.resolve(operation);
  },
  async listGitHubPullRequests() {
    await new Promise((resolve) => setTimeout(resolve, browserGitHubReadPreviewDelayMs));
    return [{
      number: 42,
      title: 'feat: add source control workspace',
      state: 'OPEN',
      draft: false,
      author: { login: 'gold-band-preview', name: 'Gold Band' },
      headRefName: 'feature/source-control',
      baseRefName: 'main',
      updatedAt: '2026-08-10T12:00:00Z',
      url: 'https://github.com/example/gold-band/pull/42',
      reviewDecision: 'REVIEW_REQUIRED',
      labels: [{ name: 'feature', color: '1d76db' }],
      statusChecks: [{ kind: 'CheckRun', name: 'test', status: 'COMPLETED', conclusion: 'SUCCESS' }],
    }];
  },
  async getGitHubPullRequest(_projectId, _workspacePath, _host, _repository, number) {
    await new Promise((resolve) => setTimeout(resolve, browserGitHubReadPreviewDelayMs));
    return {
      number,
      title: 'feat: add source control workspace',
      state: 'OPEN',
      draft: false,
      author: { login: 'gold-band-preview', name: 'Gold Band' },
      headRefName: 'feature/source-control',
      baseRefName: 'main',
      baseRefOid: '1111111111111111111111111111111111111111',
      headRefOid: '2222222222222222222222222222222222222222',
      updatedAt: '2026-08-10T12:00:00Z',
      url: `https://github.com/example/gold-band/pull/${number}`,
      reviewDecision: 'REVIEW_REQUIRED',
      labels: [{ name: 'feature', color: '1d76db' }],
      statusChecks: [{ kind: 'CheckRun', name: 'test', status: 'COMPLETED', conclusion: 'SUCCESS' }],
      body: '## Summary\n\nAdds the source control workspace and typed Git operations.',
      mergeable: 'MERGEABLE',
      mergeStateStatus: 'CLEAN',
      additions: 320,
      deletions: 18,
      changedFiles: 7,
      files: [{ path: 'src/git/source_control.rs', oldPath: null, kind: 'modified', additions: 240, deletions: 8 }],
      latestReviews: [],
    };
  },
  listGitHubIssues() {
    return Promise.resolve([{
      number: 17,
      title: 'Support repository status refresh events',
      state: 'OPEN',
      author: { login: 'contributor', name: null },
      assignees: [],
      labels: [{ name: 'enhancement', color: 'a2eeef' }],
      updatedAt: '2026-08-09T09:30:00Z',
      url: 'https://github.com/example/gold-band/issues/17',
    }]);
  },
  getGitHubIssue(_projectId, _workspacePath, _host, _repository, number) {
    return Promise.resolve({
      number,
      title: 'Support repository status refresh events',
      state: 'OPEN',
      author: { login: 'contributor', name: null },
      assignees: [],
      labels: [{ name: 'enhancement', color: 'a2eeef' }],
      updatedAt: '2026-08-09T09:30:00Z',
      url: `https://github.com/example/gold-band/issues/${number}`,
      body: 'Refresh source control state when Git metadata changes.',
      milestone: null,
    });
  },
  completeMainWindowClose() {
    return Promise.resolve();
  },
  resolveAppExit() {
    return Promise.resolve();
  },
  takePendingInterventionNavigations() {
    return Promise.resolve([]);
  },
  checkLocalClaude() {
    return Promise.resolve({ found: false, path: null });
  },
  getAppBootstrap() {
    return Promise.resolve(browserPreviewState.getAppBootstrap());
  },
  async getSystemFonts() {
    const queriedFonts = await queryBrowserLocalFonts();
    if (queriedFonts.length > 0) return queriedFonts;
    const detectedFonts = detectBrowserFonts(browserFontCandidates);
    if (detectedFonts.length > 0) return detectedFonts;
    return normalizeFontCatalogFamilies(browserFontCandidates);
  },
  getAgentRegistry() {
    return Promise.resolve(mockAgentRegistry);
  },
  getAgentCommandCatalog(agentType: string, workspacePath: string) {
    const commands = agentType === 'codex-acp'
      ? [
        { name: 'review', description: 'Review my current changes and find issues' },
        { name: 'review-branch', description: 'Review the code changes against a specific branch', inputHint: 'branch name' },
        { name: 'review-commit', description: 'Review the code changes introduced by a commit', inputHint: 'commit sha' },
        { name: 'init', description: 'Create an AGENTS.md file with instructions for Codex' },
        { name: 'compact', description: 'Summarize the conversation to preserve context' },
        { name: 'logout', description: 'Log out of Codex' },
      ]
      : [
        { name: 'pretext', description: 'Measure and lay out multiline text without DOM reflow' },
        { name: 'deep-research', description: 'Research, verify sources, and synthesize a cited report' },
        { name: 'design-sync', description: 'Push the current React design system to Claude Design', inputHint: 'project hint' },
        { name: 'update-config', description: 'Update Claude Code settings, permissions, hooks, or environment' },
        { name: 'verify', description: 'Run the app and verify that the current change works' },
        { name: 'debug', description: 'Enable debug logging and diagnose an issue', inputHint: 'issue' },
        { name: 'code-review', description: 'Review the current diff for correctness and maintainability', inputHint: 'target' },
        { name: 'simplify', description: 'Simplify and clean up the current changes' },
        { name: 'security-review', description: 'Perform a security review of the pending changes' },
        { name: 'reload-skills', description: 'Reload skills added or changed on disk' },
      ];
    return Promise.resolve({
      agentType,
      workspaceKey: workspacePath,
      updatedAt: localTimestamp(),
      commands,
    });
  },
  createAgent(_agentType: string, _input: ManagedAgentInput) {
    return Promise.resolve(mockAgentRegistry);
  },
  updateAgent(_agentType: string, _input: ManagedAgentInput) {
    return Promise.resolve(mockAgentRegistry);
  },
  deleteAgent(_agentType: string) {
    return Promise.resolve(mockAgentRegistry);
  },
  doctorAgent(_agentType: string) {
    return Promise.resolve(mockAgentRegistry);
  },
  getTaskList() {
    return Promise.resolve(mockTaskList);
  },
  getProfiles() {
    return Promise.resolve(browserPreviewState.getProfiles());
  },
  getProfile(id: string) {
    return Promise.resolve(browserPreviewState.getProfile(id) ?? browserPreviewState.getProfiles().profiles[0]);
  },
  createProfile(input: ProfileInput) {
    const now = localTimestamp();
    const profile: ProfileVm = { ...input, id: browserProfileId(), scope: 'user', isBuiltIn: false, createdAt: now, updatedAt: now, path: '' };
    return Promise.resolve(browserPreviewState.addProfile(profile));
  },
  importProfilesFromFolder(_folderPath: string, _dynamicTemplate: boolean) {
    return Promise.resolve({ totalScanned: 0, imported: [], failed: [], truncated: false });
  },
  updateProfile(id: string, input: ProfileInput) {
    const existing = browserPreviewState.getProfiles().profiles.find((profile) => profile.id === id);
    if (!existing) return browserCommandError('app.unexpected');
    if (existing.isBuiltIn) return browserCommandError('profile.readonly-built-in');
    const profile: ProfileVm = { ...existing, ...input, id, scope: 'user', isBuiltIn: false, createdAt: existing.createdAt, updatedAt: localTimestamp(), path: existing.path };
    return Promise.resolve(browserPreviewState.updateProfile(profile));
  },
  deleteProfile(id: string, force = false) {
    const existing = browserPreviewState.getProfiles().profiles.find((profile) => profile.id === id);
    if (!existing) return browserCommandError('app.unexpected');
    if (existing.isBuiltIn) return browserCommandError('profile.readonly-built-in');
    if (!force && existing.summary.includes('[requires-confirmation]')) {
      return browserCommandError('profile.delete-confirmation-required', {
        templateCount: 1,
        taskCount: 1,
        runCount: 0,
      });
    }
    return Promise.resolve(browserPreviewState.removeProfile(id));
  },
  chooseWorkspace() {
    return Promise.resolve({ ...mockBootstrap, updateBadges: browserPreviewState.getUpdateBadges() });
  },
  selectRecentWorkspace(workspace: string) {
    return Promise.resolve({ ...mockBootstrap, repoRoot: workspace, updateBadges: browserPreviewState.getUpdateBadges() });
  },
  removeRecentWorkspace(workspace: string) {
    const bootstrap = browserPreviewState.getAppBootstrap();
    if (workspace === bootstrap.repoRoot) {
      return browserCommandError('workspace.recent-current-locked', { workspace });
    }
    if (bootstrap.recentWorkspaces.length <= 1) {
      return browserCommandError('workspace.recent-minimum-required', { workspace });
    }
    return Promise.resolve(
      browserPreviewState.setRecentWorkspaces(
        bootstrap.recentWorkspaces.filter((item) => item !== workspace),
      ),
    );
  },
  getTaskDetail(taskId: string) {
    return Promise.resolve({ ...mockTaskDetail, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockTaskDetail.task });
  },
  getWorkflow(taskId: string) {
    return Promise.resolve({ ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task });
  },
  createTask(input: CreateTaskInput) {
    const task = {
      ...mockWorkflow.task,
      id: `task-${String(mockTaskList.tasks.length + 1).padStart(3, '0')}`,
      title: input.title?.trim() || `task-${String(mockTaskList.tasks.length + 1).padStart(3, '0')}`,
      description: input.description ?? null,
      requirement: input.requirementContent,
      requirementPreview: input.requirementContent.slice(0, 120),
      workflowExists: true,
      workflowValid: true,
      workflowError: null,
    };
    return Promise.resolve({ ...mockWorkflow, task, workflowJson: JSON.stringify(input.workflow, null, 2) });
  },
  saveTaskWorkflow(_projectId, taskId, workflow) {
    return Promise.resolve({ ...mockWorkflow, task: mockTaskList.tasks.find((item) => item.id === taskId) ?? mockWorkflow.task, workflowJson: JSON.stringify(workflow, null, 2) });
  },
  getWorkflowTemplates() {
    return Promise.resolve(browserPreviewState.getWorkflowTemplates());
  },
  saveWorkflowTemplate(name: string, workflow: WorkflowDsl) {
    const current = browserPreviewState.getWorkflowTemplates();
    let nextWorkflow = workflow;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const workflowId = `workflow-${crypto.randomUUID().replaceAll('-', '')}`;
      if (!current.templates.some((template) => template.workflow.id === workflowId)) {
        nextWorkflow = { ...workflow, id: workflowId };
        break;
      }
    }
    const template = {
      id: name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `workflow-${current.templates.length + 1}`,
      name,
      workflow: nextWorkflow,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: template.id,
      templates: [...current.templates, template],
    }));
  },
  updateWorkflowTemplate(templateId: string, workflow: WorkflowDsl) {
    const current = browserPreviewState.getWorkflowTemplates();
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: templateId,
      templates: current.templates.map((template) => template.id === templateId ? { ...template, workflow, updatedAt: new Date().toISOString() } : template),
    }));
  },
  deleteWorkflowTemplate(templateId: string) {
    const current = browserPreviewState.getWorkflowTemplates();
    return Promise.resolve(browserPreviewState.setWorkflowTemplates({
      ...current,
      lastUsedTemplateId: current.lastUsedTemplateId === templateId ? 'default' : current.lastUsedTemplateId,
      templates: current.templates.filter((template) => template.id !== templateId),
    }));
  },
  getAutoTemplates() {
    return Promise.resolve(browserPreviewState.getAutoTemplates());
  },
  saveAutoTemplate(name: string, config: ConversationAutoConfigVm) {
    const current = browserPreviewState.getAutoTemplates();
    let id = `auto-template-${crypto.randomUUID().replaceAll('-', '')}`;
    while (current.templates.some((template) => template.id === id)) {
      id = `auto-template-${crypto.randomUUID().replaceAll('-', '')}`;
    }
    const now = new Date().toISOString();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: [...current.templates, { id, name, config, createdAt: now, updatedAt: now }],
    }));
  },
  updateAutoTemplate(templateId: string, name: string, config: ConversationAutoConfigVm) {
    const current = browserPreviewState.getAutoTemplates();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: current.templates.map((template) => template.id === templateId ? { ...template, name, config, updatedAt: new Date().toISOString() } : template),
    }));
  },
  deleteAutoTemplate(templateId: string) {
    const current = browserPreviewState.getAutoTemplates();
    return Promise.resolve(browserPreviewState.setAutoTemplates({
      ...current,
      templates: current.templates.filter((template) => template.id !== templateId),
    }));
  },
  replaceAutoTemplates(templates: AutoTemplate[]) {
    return Promise.resolve(browserPreviewState.setAutoTemplates({ version: '0.1', templates }));
  },
  getRunDetail(taskId: string, runId: string) {
    return Promise.resolve({ ...mockRunDetail, run: { ...mockRunDetail.run, id: runId, taskId } });
  },
  getRoundDetail(taskId: string, runId: string, roundId: string, selection?: RoundSelection) {
    return Promise.resolve(mockRoundDetail(selection, { taskId, runId, roundId }));
  },
  startRun(taskId: string) {
    return Promise.resolve({ ...mockRunDetail.run, taskId });
  },
  continueRun(_projectId, taskId, runId) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  continueConversationRuntime(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ kind: 'runtime-continue-started', session: null, run: null, lifecycle: null });
  },
  recoverConversationRuntime(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _expectedRevision) {
    return Promise.reject(new Error('Browser preview does not execute workflow recovery.'));
  },
  pauseRun(taskId: string, runId: string, _projectId?: string | null) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId, status: 'paused', pauseReason: 'process-interrupted', resumable: true });
  },
  stopActiveSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({
      operationId: 'browser-preview-stop',
      status: 'accepted',
      kind: 'stop-accepted',
      run: null,
      session: fallback ?? null,
      lifecycle: null,
    });
  },
  submitManualCheck(_projectId, taskId, runId, _roundId, _nodeId, _attemptId, _outcome) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  retryRun(taskId: string, runId: string) {
    return Promise.resolve({ ...mockRunDetail.run, taskId, id: runId });
  },
  getLogPage(query: LogQueryInput) {
    return Promise.resolve(mockLogPage(query));
  },
  getAcpSession(_projectId, _taskId, runId, _roundId, _nodeId, _attemptId, query, fallback, _outerNodeId, _outerAttemptId) {
    if (runId === 'run-052' || runId === 'run-053') {
      const session = (runId === 'run-053'
        ? browserQueuedConversationRun()
        : browserCompletedConversationRun()).selectedSession;
      if (session && (!query?.branchId || session.branchId === query.branchId)) {
        return Promise.resolve(session);
      }
    }
    return Promise.resolve(fallback ?? null);
  },
  getAcpActivityDetail() {
    return Promise.resolve({ items: [], hasMoreEarlier: false, earlierCursor: null });
  },
  getAcpToolDetail() {
    return Promise.resolve({ event: null });
  },
  getTurnFileChangeSet(locator, changeSetId) {
    if (changeSetId === browserTurnFileChangeSet.id) {
      return Promise.resolve({ ...browserTurnFileChangeSet, branchId: locator.branchId });
    }
    return Promise.resolve({
      id: changeSetId,
      turnId: 'browser-turn',
      promptEventId: 'browser-prompt',
      branchId: locator.branchId,
      status: 'finalized' as const,
      startedAt: '',
      finishedAt: '',
      summary: { fileCount: 0, addedFiles: 0, modifiedFiles: 0, deletedFiles: 0, addedLines: 0, deletedLines: 0 },
      changes: [],
      limitationCodes: [],
    });
  },
  getFileComparison(_locator, changeSetId, changeId) {
    if (changeSetId === browserTurnFileChangeSet.id && changeId === 'browser-added-readme') {
      return Promise.resolve({
        changeSetId,
        changeId,
        path: 'docs/workspace-notes.md',
        stats: { addedLines: 4, deletedLines: 0 },
        before: null,
        after: {
          version: { id: 'browser-added-version', storageKind: 'capturedBlob' as const, contentHash: 'browser-added', byteLength: 69, encoding: 'utf-8', lineEnding: 'lf' },
          content: '# Workspace notes\n\n- Use captured tool-call output.\n- Keep history read-only.\n',
        },
        limitationCode: null,
      });
    }
    if (changeSetId === browserTurnFileChangeSet.id && changeId === 'browser-modified-config') {
      return Promise.resolve({
        changeSetId,
        changeId,
        path: 'src/config.json',
        stats: { addedLines: 4, deletedLines: 2 },
        before: {
          version: { id: 'browser-before-version', storageKind: 'capturedBlob' as const, contentHash: 'browser-before', byteLength: 29, encoding: 'utf-8', lineEnding: 'lf' },
          content: '{\n  "workspace": "default"\n}\n',
        },
        after: {
          version: { id: 'browser-after-version', storageKind: 'capturedBlob' as const, contentHash: 'browser-after', byteLength: 72, encoding: 'utf-8', lineEnding: 'lf' },
          content: '{\n  "workspace": "default",\n  "history": "captured",\n  "readOnly": true\n}\n',
        },
        limitationCode: null,
      });
    }
    return Promise.resolve({
      changeSetId,
      changeId,
      path: '',
      stats: { addedLines: 0, deletedLines: 0 },
      before: null,
      after: null,
      limitationCode: null,
    });
  },
  subscribeAcpSessionUpdates() {
    return Promise.resolve(() => {});
  },
  subscribeConversationRunStateUpdates() {
    return Promise.resolve(() => {});
  },
  subscribeInterventionNavigate() {
    return Promise.resolve(() => {});
  },
  subscribeAppExitRequested() {
    return Promise.resolve(() => {});
  },
  submitConversationPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _input, _promptId, fallback, _outerNodeId, _outerAttemptId, _attachmentPaths) {
    return Promise.resolve({ kind: 'acp-session', session: fallback ?? null, run: null });
  },
  reorderConversationQueuedPrompts(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _expectedRevision, _orderedItemIds, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ lifecycle: null });
  },
  restoreConversationQueuedPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, itemId, _outerNodeId, _outerAttemptId) {
    const item = browserQueuedPromptDrafts.find((candidate) => candidate.id === itemId);
    if (!item) return Promise.reject(new Error('queued prompt not found'));
    return Promise.resolve({
      draft: {
        content: item.content,
        quotes: item.quotes.map((quote) => ({ ...quote })),
        attachmentPaths: [...item.attachmentPaths],
      },
      lifecycle: null,
    });
  },
  deleteConversationQueuedPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _itemId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ lifecycle: null });
  },
  useConversationQueuedPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _itemId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ kind: 'acp-session', session: null, run: null, lifecycle: null });
  },
  sendAcpPrompt(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _prompt, _promptId, fallback, _outerNodeId, _outerAttemptId, _attachmentPaths) {
    return Promise.resolve(fallback ?? null);
  },
  setAcpSessionModel(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _modelId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  setAcpSessionPermissionMode(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _permissionModeId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  setAcpSessionConfigOption(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _optionId, _optionValue, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(null);
  },
  respondAcpPermission(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _requestId, _optionId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(fallback ?? null);
  },
  respondElicitation(_projectId: string | null | undefined, _taskId: string, _runId: string, _roundId: string, _nodeId: string, _attemptId: string, _elicitationId: string, _action: string, _content?: Record<string, unknown> | null, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return Promise.resolve();
  },
  cancelAcpSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, fallback, _outerNodeId, _outerAttemptId) {
    return Promise.resolve(fallback ?? null);
  },
  getAcpRawFrames(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, query, _outerNodeId, _outerAttemptId) {
    const empty: AcpRawFramePageVm = {
      items: [],
      page: query?.page ?? 0,
      pageSize: query?.pageSize ?? 100,
      total: 0,
      hasPrevious: false,
      hasNext: false,
      order: query?.order ?? 'desc',
      search: query?.search ?? null,
      kind: query?.kind ?? null,
      direction: query?.direction ?? null,
    };
    return Promise.resolve(empty);
  },
  showArtifact(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, name, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ ...mockContent, title: name });
  },
  showAttachment(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, name, _outerNodeId, _outerAttemptId) {
    return Promise.resolve({ ...mockContent, title: name, kind: 'attachment' });
  },
  showConversationAttachment(_projectId: string, _taskId: string, name: string) {
    if (/\.(png|jpe?g|webp|gif|bmp)$/i.test(name)) {
      return Promise.resolve({
        ...mockContent,
        title: name,
        kind: 'input-attachment',
        content: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=',
        metadata: { mimeType: 'image/png', isImage: true, encoding: 'data-url' },
      });
    }
    return Promise.resolve({ ...mockContent, title: name, kind: 'input-attachment' });
  },
  showConversationMessageAttachment(_projectId: string, _taskId: string, _runId: string, _roundId: string, _nodeId: string, _attemptId: string, name: string, _path: string, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return this.showConversationAttachment(_projectId, _taskId, name);
  },
  showWorkerRef(_taskId: string, _runId: string, _roundId: string, _nodeId: string, attemptId: string, _outerNodeId?: string | null, _outerAttemptId?: string | null) {
    return Promise.resolve({ ...mockContent, title: attemptId, kind: 'worker-ref' });
  },
  saveDesktopPreferences(appearance: AppearancePreference, personalization: PersonalizationPreference, language: DesktopLanguage, useLocalClaude: boolean, verboseLogging: boolean) {
    const current = browserPreviewState.getPreferences();
    const preferences = browserPreviewState.setPreferences({ ...current, appearance, personalization, language, useLocalClaude, verboseLogging });
    return Promise.resolve(preferences);
  },
  saveDesktopAvatar(input) {
    const current = browserPreviewState.getPreferences();
    const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `avatar-${Date.now()}`;
    const profile = current.avatars[input.kind];
    const recentAvatars = [
      { id, dataUrl: `data:${input.mimeType};base64,${input.dataBase64}`, createdAt: localTimestamp() },
      ...profile.recentAvatars.filter((avatar) => avatar.id !== id),
    ].slice(0, 10);
    const avatars = {
      ...current.avatars,
      [input.kind]: {
        shape: input.shape,
        selectedAvatarId: id,
        recentAvatars,
      },
    };
    const personalization = {
      ...current.personalization,
      avatars: {
        ...current.personalization.avatars,
        [input.kind]: {
          image: { source: 'user' as const, assetId: id },
          shape: { source: 'custom' as const, value: input.shape },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, avatars }));
  },
  selectRecentDesktopAvatar(kind, avatarId) {
    const current = browserPreviewState.getPreferences();
    const profile = current.avatars[kind];
    const selected = profile.recentAvatars.find((avatar) => avatar.id === avatarId);
    if (!selected) return Promise.reject({ code: 'avatar.recent-not-found', params: { avatarId } });
    const avatars = {
      ...current.avatars,
      [kind]: {
        ...profile,
        selectedAvatarId: avatarId,
        recentAvatars: [selected, ...profile.recentAvatars.filter((avatar) => avatar.id !== avatarId)],
      },
    };
    const personalization = {
      ...current.personalization,
      avatars: {
        ...current.personalization.avatars,
        [kind]: { ...current.personalization.avatars[kind], image: { source: 'user' as const, assetId: avatarId } },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, avatars }));
  },
  saveDesktopAvatarShape(kind, shape) {
    const current = browserPreviewState.getPreferences();
    const effectiveShape = shape ?? 'circle';
    const avatars = {
      ...current.avatars,
      [kind]: { ...current.avatars[kind], shape: effectiveShape },
    };
    const personalization = {
      ...current.personalization,
      avatars: {
        ...current.personalization.avatars,
        [kind]: {
          ...current.personalization.avatars[kind],
          shape: shape === null ? { source: 'theme' as const } : { source: 'custom' as const, value: shape },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, avatars }));
  },
  clearDesktopAvatar(kind) {
    const current = browserPreviewState.getPreferences();
    const avatars = {
      ...current.avatars,
      [kind]: { ...current.avatars[kind], selectedAvatarId: null },
    };
    const personalization = {
      ...current.personalization,
      avatars: {
        ...current.personalization.avatars,
        [kind]: { ...current.personalization.avatars[kind], image: { source: 'theme' as const } },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, avatars }));
  },
  importDesktopWallpaper(colorScheme) {
    const current = browserPreviewState.getPreferences();
    const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `wallpaper-${Date.now()}`;
    const imageUrl = 'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900"%3E%3Cdefs%3E%3ClinearGradient id="g" x2="1" y2="1"%3E%3Cstop stop-color="%23111927"/%3E%3Cstop offset=".5" stop-color="%230f766e"/%3E%3Cstop offset="1" stop-color="%23d4a72c"/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width="1600" height="900" fill="url(%23g)"/%3E%3C/svg%3E';
    const wallpaper = {
      id,
      imageUrl,
      thumbnailUrl: imageUrl,
      createdAt: localTimestamp(),
      width: 1600,
      height: 900,
    };
    const retainedAssetIds = Object.values(current.personalization.wallpaper.byColorScheme)
      .flatMap((preference) => preference.image.source === 'user' ? [preference.image.assetId] : []);
    const wallpapers = {
      recentWallpapers: boundedRecentWallpapers(
        [wallpaper, ...current.wallpapers.recentWallpapers],
        retainedAssetIds,
      ),
    };
    const personalization = {
      ...current.personalization,
      wallpaper: {
        ...current.personalization.wallpaper,
        byColorScheme: {
          ...current.personalization.wallpaper.byColorScheme,
          [colorScheme]: {
            ...current.personalization.wallpaper.byColorScheme[colorScheme],
            image: { source: 'user' as const, assetId: id },
          },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, wallpapers }));
  },
  selectRecentDesktopWallpaper(colorScheme, wallpaperId) {
    const current = browserPreviewState.getPreferences();
    const selected = current.wallpapers.recentWallpapers.find((wallpaper) => wallpaper.id === wallpaperId);
    if (!selected) return Promise.reject({ code: 'wallpaper.recent-not-found', params: { wallpaperId } });
    const wallpapers = {
      recentWallpapers: [selected, ...current.wallpapers.recentWallpapers.filter((wallpaper) => wallpaper.id !== wallpaperId)],
    };
    const personalization = {
      ...current.personalization,
      wallpaper: {
        ...current.personalization.wallpaper,
        byColorScheme: {
          ...current.personalization.wallpaper.byColorScheme,
          [colorScheme]: {
            ...current.personalization.wallpaper.byColorScheme[colorScheme],
            image: { source: 'user' as const, assetId: wallpaperId },
          },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization, wallpapers }));
  },
  saveDesktopWallpaperOpacity(colorScheme, opacityPercent) {
    const current = browserPreviewState.getPreferences();
    const personalization = {
      ...current.personalization,
      wallpaper: {
        ...current.personalization.wallpaper,
        byColorScheme: {
          ...current.personalization.wallpaper.byColorScheme,
          [colorScheme]: {
            ...current.personalization.wallpaper.byColorScheme[colorScheme],
            opacityPercent,
          },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization }));
  },
  restoreThemeDesktopWallpaper(colorScheme) {
    const current = browserPreviewState.getPreferences();
    const personalization = {
      ...current.personalization,
      wallpaper: {
        ...current.personalization.wallpaper,
        byColorScheme: {
          ...current.personalization.wallpaper.byColorScheme,
          [colorScheme]: {
            ...current.personalization.wallpaper.byColorScheme[colorScheme],
            image: { source: 'theme' as const },
          },
        },
      },
    };
    return Promise.resolve(browserPreviewState.setPreferences({ ...current, personalization }));
  },
  saveUpdaterSettings(overrideUrl: string | null) {
    const current = browserPreviewState.getUpdaterSettings();
    const normalized = overrideUrl?.trim() ? overrideUrl.trim() : null;
    return Promise.resolve(browserPreviewState.setUpdaterSettings({
      ...current,
      overrideUrl: normalized,
      effectiveUrl: normalized ?? current.builtInUrl,
    }));
  },
  updateNotificationAttention(_input) {
    return Promise.resolve();
  },
  getMetricsSettings() {
    return Promise.resolve({
      enabled: false,
      toggleLocked: false,
      metricsBaseUrl: null,
      heartbeatEndpoint: null,
      nodeMetricsEndpoint: null,
      apiKeySet: false,
    });
  },
  saveMetricsSettings(_enabled: boolean, _metricsBaseUrl: string | null, _apiKey: string | null) {
    return this.getMetricsSettings();
  },
  getUpdateStatus() {
    return Promise.resolve(browserPreviewState.getUpdateStatus());
  },
  markSettingsUpdateSeen(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, settingsEntrySeenVersion: version }));
  },
  markSettingsAdvancedUpdateSeen(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, settingsAdvancedSeenVersion: version }));
  },
  dismissUpdateAnnouncement(version: string) {
    const current = browserPreviewState.getUpdateBadges();
    return Promise.resolve(browserPreviewState.setUpdateBadges({ ...current, announcementClosedVersion: version }));
  },
  checkUpdateManual() {
    return Promise.resolve(browserPreviewState.setUpdateStatus({
      status: 'error',
      checkedAt: localTimestamp(),
      update: null,
      error: { code: 'updater.check-failed', params: { message: 'Browser preview cannot check desktop updates.' } },
      background: false,
    }));
  },
  downloadAndInstallUpdate() {
    return Promise.resolve();
  },
  // ── Conversation UI mocks ──
  saveDesktopUiMode(_mode) {
    return Promise.resolve();
  },
  getConversationSidebar() {
    const sidebar: ConversationSidebarVm = {
      workspaces: [{ projectId: 'default', workspacePath: '/default', name: 'Default Workspace' }],
      pinnedTasks: [],
      tasksByWorkspace: { default: [] },
    };
    return Promise.resolve(sidebar);
  },
  listScheduledTasks(projectId) {
    return Promise.resolve(browserScheduledTasks
      .filter((task) => !projectId || task.projectId === projectId)
      .map((task) => ({ ...task })));
  },
  setScheduledTaskEnabled(_projectId, scheduledTaskId, enabled) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId);
    if (task) {
      task.enabled = enabled;
      task.status = enabled ? 'enabled' : 'paused';
      task.updatedAt = new Date().toISOString();
      emitBrowserScheduledTaskUpdated(task);
      return Promise.resolve({ ...task });
    }
    return browserCommandError('scheduled-task.not-found');
  },
  createScheduledTask(input) {
    const now = new Date().toISOString();
    const id = `scheduled-${Date.now()}-${++browserScheduledTaskSequence}`;
    const schedule = scheduledScheduleSpecFromInput(input.schedule);
    const task: ScheduledTaskVm = { id, projectId: input.projectId, workspaceName: input.projectId === 'default' ? 'Default Workspace' : input.projectId, title: input.content.split(/\r?\n/)[0].slice(0, 48), enabled: true, mode: input.runMode, sessionPolicy: input.sessionPolicy ?? 'new', schedule: structuredClone(schedule), nextAt: null, status: 'enabled', lastTriggerAt: null, lastTriggerStatus: null, createdAt: now, updatedAt: now };
    const definition: ScheduledTaskEditVm = {
      scheduledTaskId: id,
      projectId: input.projectId,
      content: input.content,
      attachmentNames: [],
      runMode: input.runMode,
      workflowTemplateId: input.workflowTemplateId,
      includeInterview: input.includeInterview,
      directConfig: input.directConfig,
      autoConfig: input.autoConfig,
      schedule,
      overlapPolicy: input.overlapPolicy,
      sessionPolicy: input.sessionPolicy ?? 'new',
      directAgentType: input.directConfig?.agentType ?? null,
      expectedUpdatedAt: now,
    };
    browserScheduledTaskDefinitions.set(id, definition);
    browserScheduledOccurrences.set(id, []);
    browserScheduledTasks.push(task);
    emitBrowserScheduledTaskUpdated(task);
    return Promise.resolve({ ...task });
  },
  getScheduledTask(_projectId, scheduledTaskId) {
    const definition = browserScheduledTaskDefinitions.get(scheduledTaskId);
    return definition ? Promise.resolve(structuredClone(definition)) : browserCommandError('scheduled-task.not-found');
  },
  updateScheduledTask(input: UpdateScheduledTaskInput) {
    const definition = browserScheduledTaskDefinitions.get(input.scheduledTaskId);
    if (!definition) return browserCommandError('scheduled-task.not-found');
    if (definition.expectedUpdatedAt !== input.expectedUpdatedAt) return browserCommandError('scheduled-task.conflict');
    const now = new Date().toISOString();
    const schedule = scheduledScheduleSpecFromInput(input.schedule);
    const next: ScheduledTaskEditVm = {
      ...definition,
      ...input,
      schedule,
      expectedUpdatedAt: now,
      directAgentType: input.directConfig?.agentType ?? definition.directAgentType ?? null,
    };
    browserScheduledTaskDefinitions.set(input.scheduledTaskId, next);
    const task = browserScheduledTasks.find((item) => item.id === input.scheduledTaskId);
    if (task) {
      Object.assign(task, {
        title: input.content.split(/\r?\n/)[0].slice(0, 48),
        mode: input.runMode,
        sessionPolicy: input.sessionPolicy,
        schedule: structuredClone(schedule),
        updatedAt: now,
      });
      emitBrowserScheduledTaskUpdated(task);
    }
    return Promise.resolve(structuredClone(next));
  },
  deleteScheduledTask(_projectId, scheduledTaskId) {
    const index = browserScheduledTasks.findIndex((task) => task.id === scheduledTaskId);
    if (index < 0) return browserCommandError('scheduled-task.not-found');
    const [task] = browserScheduledTasks.splice(index, 1);
    browserScheduledTaskDefinitions.delete(scheduledTaskId);
    browserScheduledOccurrences.delete(scheduledTaskId);
    emitBrowserScheduledTaskUpdated({ ...task, status: 'deleted' });
    return Promise.resolve();
  },
  listScheduledTaskOccurrences(projectId, scheduledTaskId, limit = 50) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    return Promise.resolve((browserScheduledOccurrences.get(scheduledTaskId) ?? []).slice(0, Math.max(1, Math.min(limit, 200))).map((occurrence) => structuredClone(occurrence)));
  },
  getScheduledTaskDiagnostics(projectId, scheduledTaskId) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    const occurrences = browserScheduledOccurrences.get(scheduledTaskId) ?? [];
    const last = occurrences[0] ?? null;
    return Promise.resolve({
      scheduledTaskId,
      projectId,
      nextAt: task.nextAt ?? null,
      lastStatus: last?.status ?? task.lastTriggerStatus ?? null,
      lastError: last?.errorCode ?? null,
      runCount: occurrences.filter((occurrence) => Boolean(occurrence.runId)).length,
      retryCount: occurrences.reduce((count, occurrence) => count + Math.max(0, occurrence.attempt - 1), 0),
      occurrences: occurrences.slice(0, 200).map((occurrence) => structuredClone(occurrence)),
    });
  },
  runScheduledTaskNow(projectId, scheduledTaskId) {
    const task = browserScheduledTasks.find((item) => item.id === scheduledTaskId && item.projectId === projectId);
    if (!task) return browserCommandError('scheduled-task.not-found');
    const now = new Date().toISOString();
    const occurrenceId = `occurrence-${Date.now()}-${++browserScheduledTaskSequence}`;
    const taskId = `browser-task-${scheduledTaskId}`;
    const runId = `browser-run-${Date.now()}-${browserScheduledTaskSequence}`;
    const running: ScheduledOccurrenceVm = {
      id: occurrenceId,
      scheduledTaskId,
      scheduledAt: now,
      triggerKind: 'manual',
      status: 'running',
      attempt: 1,
      errorCode: null,
      errorParams: null,
      taskId,
      runId,
      roundId: null,
      attemptId: null,
      startedAt: now,
      finishedAt: null,
    };
    const history = browserScheduledOccurrences.get(scheduledTaskId) ?? [];
    browserScheduledOccurrences.set(scheduledTaskId, [running, ...history]);
    emitBrowserScheduledOccurrenceUpdated(running, task.projectId);
    const finished: ScheduledOccurrenceVm = { ...running, status: 'succeeded', finishedAt: new Date().toISOString() };
    browserScheduledOccurrences.set(scheduledTaskId, [finished, ...history]);
    Object.assign(task, {
      lastTriggerAt: finished.finishedAt,
      lastTriggerStatus: finished.status,
      updatedAt: finished.finishedAt ?? now,
    });
    emitBrowserScheduledOccurrenceUpdated(finished, task.projectId);
    emitBrowserScheduledTaskUpdated(task);
    return Promise.resolve({
      occurrence: structuredClone(finished),
      taskId,
      runId,
      roundId: null,
      attemptId: null,
    } satisfies RunScheduledTaskResultVm);
  },
  getConversationWorkspaces() {
    return Promise.resolve([{ projectId: 'default', workspacePath: '/default', name: 'Default Workspace' }]);
  },
  getConversationRun(_projectId, _taskId, runId) {
    if (runId === 'run-051') return Promise.resolve(mockErrorBlockedConversationRun);
    if (runId === 'run-052') return Promise.resolve(browserCompletedConversationRun());
    if (runId === 'run-053') return Promise.resolve(browserQueuedConversationRun());
    const created = browserConversationRuns.get(runId);
    if (created) return Promise.resolve(created);
    const run: ConversationRunVm = {
      projectId: 'default',
      taskId: 'mock-task',
      runId,
      title: 'Mock Task',
      autoTitle: true,
      runMode: 'auto',
      runStatus: 'completed',
      sessionTree: { rounds: [], selectedSessionKey: null },
      selectedSession: null,
      activeSessions: [],
      inputAttachments: [],
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      runtimeErrorMessage: null,
    };
    return Promise.resolve(run);
  },
  switchConversationSession(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _outerNodeId, _outerAttemptId) {
    if (_runId === 'run-051') return Promise.resolve({ selectedSession: mockErrorBlockedConversationSession });
    if (_runId === 'run-052') return Promise.resolve({ selectedSession: browserCompletedConversationRun().selectedSession });
    if (_runId === 'run-053') return Promise.resolve({ selectedSession: browserQueuedConversationRun().selectedSession });
    return Promise.resolve({ selectedSession: null });
  },
  validateConversationCreate(_input) {
    return Promise.resolve({ valid: true, missingItems: [] });
  },
  createConversationRun(input) {
    const run: ConversationRunVm = {
      projectId: input.projectId,
      taskId: `task-${Date.now()}`,
      runId: `run-${Date.now()}`,
      title: input.content.slice(0, 12) || 'New Task',
      autoTitle: true,
      runMode: input.runMode,
      directConfig: input.directConfig,
      agentIdentity: input.directConfig ? browserAgentIdentity(input.directConfig.agentType) : null,
      lastActivityAt: new Date().toISOString(),
      runStatus: 'running',
      sessionTree: { rounds: [], selectedSessionKey: null },
      selectedSession: null,
      activeSessions: [],
      inputAttachments: [],
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      runtimeErrorMessage: null,
    };
    browserConversationRuns.set(run.runId, run);
    return Promise.resolve(run);
  },
  rerunConversationTask(_projectId, _taskId) {
    return this.createConversationRun({ projectId: _projectId, content: 'Rerun', runMode: 'auto' });
  },
  updateTaskMetadata() {
    return Promise.resolve();
  },
  deleteConversationTask(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  pinConversation(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  unpinConversation(_projectId, _taskId) {
    return this.getConversationSidebar();
  },
  reorderPinnedConversations(_pins) {
    return this.getConversationSidebar();
  },
  searchConversationTasks(_query, _limit) {
    return Promise.resolve([]);
  },
  getConversationRunMode(_projectId) {
    return Promise.resolve({ mode: 'auto' });
  },
  saveConversationRunMode() {
    return Promise.resolve();
  },
  chooseConversationWorkspace() {
    const ws: ConversationWorkspaceVm = { projectId: 'default', workspacePath: '/default', name: 'Default Workspace' };
    return Promise.resolve(ws);
  },
  addConversationWorkspace() {
    return this.getConversationSidebar();
  },
  removeConversationWorkspace(_projectId) {
    return this.getConversationSidebar();
  },
  syncConversationWorkspace(_workspacePath) {
    return this.getConversationSidebar();
  },
  saveConversationPreference(_key, _value) {
    return Promise.resolve();
  },
  saveLastConversationWorkspace(_projectId) {
    return Promise.resolve();
  },
  listWorkspaceDirectory(_projectId, relativePath) {
    return Promise.resolve(browserDirectoryEntries(relativePath));
  },
  openWorkspacePathInFileManager(_projectId, _relativePath = '') {
    return Promise.resolve();
  },
  listConversationDirectory(_input) { return Promise.resolve([]); },
  openConversationDirectoryPathInFileManager(_input) { return Promise.resolve(); },
  readConversationDirectoryFile(_input) { return Promise.reject(new Error('conversation-directory.unavailable')); },
  searchWorkspaceFiles(_projectId, query, requestId, limit) {
    const normalized = query.trim().toLocaleLowerCase();
    const matches = [...browserWorkspaceFiles.entries()]
      .filter(([path]) => path.toLocaleLowerCase().includes(normalized))
      .slice(0, limit)
      .map(([path, content]) => ({
        name: path.split('/').at(-1) ?? path,
        relativePath: browserRelativePath(path) ?? path,
        canonicalPath: path,
        kind: 'file' as const,
        hasChildren: false,
        byteLength: new TextEncoder().encode(content).byteLength,
        modifiedAtNs: String(browserFileRevisions.get(path) ?? 0),
      }));
    return Promise.resolve({ requestId, entries: matches, truncated: matches.length >= limit });
  },
  resolveWorkspaceFileLink(projectId, rawHref, baseCanonicalPath = null) {
    let href = decodeURIComponent(rawHref.replace(/^file:\/\//u, ''));
    let line: number | null = null;
    let column: number | null = null;
    const fragment = href.match(/#L(\d+)(?:-L?(\d+))?$/iu);
    const endLine = fragment?.[2] ? Number(fragment[2]) : null;
    if (fragment) {
      line = Number(fragment[1]);
      href = href.slice(0, fragment.index);
    } else {
      const suffix = href.match(/:(\d+)(?::(\d+))?$/u);
      if (suffix) {
        line = Number(suffix[1]);
        column = suffix[2] ? Number(suffix[2]) : null;
        href = href.slice(0, suffix.index);
      }
    }
    const normalizedHref = href.replaceAll('\\', '/');
    const baseDirectory = baseCanonicalPath
      ? baseCanonicalPath.replaceAll('\\', '/').replace(/\/[^/]*$/u, '')
      : browserWorkspaceRoot;
    const canonicalPath = href.startsWith('/') || /^[A-Za-z]:[\\/]/u.test(href)
      ? normalizedHref
      : new URL(normalizedHref, `file:///${baseDirectory.replace(/^\/+/, '')}/`).pathname.replace(/^\/([A-Za-z]:)/u, '$1');
    const relativePath = browserRelativePath(canonicalPath);
    const externalAccessGrant = relativePath == null ? issueBrowserExternalFileGrant(canonicalPath) : null;
    if (!browserWorkspaceFiles.has(canonicalPath)) {
      browserWorkspaceFiles.set(canonicalPath, '# External file\n\nBrowser preview content.\n');
    }
    return Promise.resolve({
      locator: { projectId, canonicalPath, relativePath, scope: relativePath == null ? 'external' as const : 'workspace' as const },
      target: line ? { line, column, endLine } : null,
      externalAccessGrant,
    });
  },
  readFileResource(projectId, canonicalPath, externalAccessToken = null, preferSource = false) {
    const content = browserWorkspaceFiles.get(canonicalPath);
    if (content == null) return Promise.reject({ code: 'workspace-file.not-found', params: { path: canonicalPath } });
    const relativePath = browserRelativePath(canonicalPath);
    if (relativePath == null && !browserExternalGrantValid(externalAccessToken, canonicalPath)) {
      return Promise.reject({ code: 'workspace-file.external-access-denied', params: { path: canonicalPath } });
    }
    const externalAccessGrant = relativePath == null && externalAccessToken
      ? {
          token: externalAccessToken,
          permissions: ['read', 'write'] as Array<'read' | 'write'>,
          expiresAtMs: String(browserExternalFileGrants.get(externalAccessToken)?.expiresAtMs ?? Date.now()),
        }
      : null;
    const locator = { projectId, canonicalPath, relativePath, scope: relativePath == null ? 'external' as const : 'workspace' as const };
    const name = canonicalPath.split('/').at(-1) ?? canonicalPath;
    const revision = browserFileRevision(canonicalPath, content);
    if (canonicalPath.toLocaleLowerCase().endsWith('.svg') && !preferSource) {
      return Promise.resolve({
        kind: 'image' as const,
        locator,
        name,
        revision,
        mimeType: 'image/svg+xml',
        width: 240,
        height: 120,
        animated: false,
        previewGrant: {
          token: `browser-preview:${canonicalPath}`,
          expiresAtMs: String(Date.now() + 5 * 60 * 1_000),
        },
        sourceEditable: true,
        externalAccessGrant,
      });
    }
    return Promise.resolve({
      kind: 'text' as const,
      locator,
      name,
      revision,
      content,
      encoding: 'utf-8',
      language: canonicalPath.endsWith('.rs') ? 'rust' : canonicalPath.endsWith('.json') ? 'json' : canonicalPath.endsWith('.svg') ? 'xml' : 'markdown',
      lineEnding: 'lf' as const,
      editable: true,
      limitationCode: null,
      externalAccessGrant,
    });
  },
  resolveMarkdownImage(input) {
    const raw = decodeURIComponent(input.rawSrc).replaceAll('\\', '/');
    if (/^(?:https?:|data:|javascript:)/iu.test(raw)) {
      return Promise.reject({ code: 'workspace-file.markdown-image-network-blocked', params: { src: raw } });
    }
    const parent = input.markdownCanonicalPath.replace(/\/[^/]*$/u, '');
    const canonicalPath = /^[A-Za-z]:\//u.test(raw) || raw.startsWith('/')
      ? raw
      : `${parent}/${raw}`.replace('/./', '/');
    return Promise.resolve({
      kind: 'ready' as const,
      canonicalPath,
      previewGrant: {
        token: `browser-preview:${canonicalPath}`,
        expiresAtMs: String(Date.now() + 5 * 60 * 1_000),
      },
      mimeType: canonicalPath.toLowerCase().endsWith('.svg') ? 'image/svg+xml' : 'image/png',
      width: 640,
      height: 360,
      animated: false,
    });
  },
  writeFileResource(input) {
    const current = browserWorkspaceFiles.get(input.canonicalPath);
    if (current == null) return Promise.reject({ code: 'workspace-file.not-found', params: { path: input.canonicalPath } });
    const currentRevision = browserFileRevision(input.canonicalPath, current);
    if (browserRelativePath(input.canonicalPath) == null
      && !browserExternalGrantValid(input.externalAccessToken, input.canonicalPath)) {
      return Promise.reject({ code: 'workspace-file.external-access-denied', params: { path: input.canonicalPath } });
    }
    if (!input.force && currentRevision.contentHash !== input.expectedRevision.contentHash) {
      return Promise.reject({ code: 'workspace-file.changed-on-disk', params: { path: input.canonicalPath } });
    }
    browserWorkspaceFiles.set(input.canonicalPath, input.content);
    browserFileRevisions.set(input.canonicalPath, (browserFileRevisions.get(input.canonicalPath) ?? 0) + 1);
    const revision = browserFileRevision(input.canonicalPath, input.content);
    for (const listener of browserWorkspaceFileListeners) {
      listener({ projectId: input.projectId, canonicalPath: input.canonicalPath, kind: 'modified', revision, operationId: input.operationId });
    }
    return Promise.resolve(revision);
  },
  releaseWorkspaceFilePreview(_token) {
    return Promise.resolve();
  },
  renewExternalFileAccess(token) {
    const grant = browserExternalFileGrants.get(token);
    if (!grant || grant.expiresAtMs <= Date.now()) {
      return Promise.reject({ code: 'workspace-file.external-access-denied', params: { operation: 'renew' } });
    }
    browserExternalFileGrants.delete(token);
    return Promise.resolve(issueBrowserExternalFileGrant(grant.canonicalPath));
  },
  releaseExternalFileAccess(token) {
    browserExternalFileGrants.delete(token);
    return Promise.resolve();
  },
  startWorkspaceFileWatch(_projectId) {
    return Promise.resolve();
  },
  stopWorkspaceFileWatch(_projectId) {
    return Promise.resolve();
  },
  subscribeWorkspaceFileChanges(listener) {
    browserWorkspaceFileListeners.add(listener);
    return Promise.resolve(() => browserWorkspaceFileListeners.delete(listener));
  },
  workspaceFilePreviewUrl(token, _staticFrame = false) {
    const path = token.replace(/^browser-preview:/u, '');
    return browserSvgDataUrl(browserWorkspaceFiles.get(path) ?? '<svg xmlns="http://www.w3.org/2000/svg"/>');
  },
  openExternalUrl(url) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return Promise.resolve();
  },
  openFileWithSystemApp(_path) {
    return Promise.resolve();
  },
  pickAttachmentFiles() {
    return Promise.resolve([]);
  },
  statAttachmentFiles(paths) {
    return Promise.resolve(paths.map((path) => ({
      path,
      name: path.split(/[\\/]/).at(-1) ?? path,
      size: 0,
      previewUrl: null,
    })));
  },
  materializeConversationAttachments(files) {
    return Promise.resolve(files.map((file, index) => ({
      path: `browser-memory://attachments/${Date.now()}-${index}-${encodeURIComponent(file.name)}`,
      name: file.name,
      size: file.size,
    })));
  },
  getSupportedAttachmentExtensions() {
    return Promise.resolve([
      "png", "jpg", "jpeg", "webp", "gif", "bmp",
      "txt", "md", "json", "jsonl", "csv",
      "html", "htm", "css", "js", "ts", "tsx", "jsx",
      "rs", "py", "go", "java", "c", "h", "cpp", "hpp",
      "yaml", "yml", "xml", "toml", "log", "sql", "sh", "bash", "zsh",
    ]);
  },
  openInFileManager(_projectId, _taskId, _runId, _roundId, _nodeId, _attemptId, _outerNodeId, _outerAttemptId) {
    return Promise.resolve();
  },
  // MCP & SKILL stubs
  listMcpServers() { return Promise.resolve([]); },
  addMcpServer(_jsonContent: string) { return Promise.resolve([]); },
  updateMcpServer(_id: string, _jsonContent: string) { return Promise.resolve([]); },
  deleteMcpServer(_id: string) { return Promise.resolve([]); },
  toggleMcpServer(_id: string, _enabled: boolean) { return Promise.resolve([]); },
  checkMcpServerHealth(_id: string) { return Promise.resolve({ status: 'unknown' }); },
  listMcpTools(_id: string) { return Promise.resolve([]); },
  listSkills() { return Promise.resolve({ global: [], project: [] }); },
  listProjectSkills(_workspacePath: string) { return Promise.resolve([]); },
  readSkill(_name: string, _source: string, _workspacePath?: string | null, _directoryPath?: string | null) { return Promise.resolve({ meta: { name: '', description: '', source: 'global' as const, directoryPath: '', agentSource: '.gold-band', loadWarnings: [], syncedAgentTypes: [] }, body: '' }); },
  writeSkill(_name: string, _source: string, _content: string, _workspacePath?: string | null, _oldName?: string | null, _directoryPath?: string | null, _syncTargets?: string[] | null) { return Promise.resolve({ global: [], project: [] }); },
  deleteSkill(_name: string, _source: string) { return Promise.resolve({ global: [], project: [] }); },
  updateSkillSyncTargets(_name: string, _source: string, _workspacePath: string | null | undefined, _directoryPath: string, _syncTargets: string[]) { return Promise.resolve({ global: [], project: [] }); },
  getSkillSyncStatus(_name: string, _directoryPath: string, _workspacePath?: string | null) { return Promise.resolve([]); },
  checkSkillNameConflict(_name: string, _source: string, _workspacePath?: string | null, _oldName?: string | null, _directoryPath?: string | null, _syncTargets?: string[] | null) { return Promise.resolve([] as string[]); },
  submitFeedback(_input: import('../types').FeedbackInput): Promise<import('../types').FeedbackResult> {
    return Promise.reject({ code: 'feedback.endpoint-unconfigured', params: {} });
  },
  previewFeedbackSessionArchive(): Promise<null> {
    return Promise.resolve(null);
  },
};

function browserProfileId() {
  return `pf-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function browserCommandError(code: string, params: Record<string, unknown> = {}) {
  return Promise.reject({ code, params });
}

function detectBrowserFonts(candidates: readonly string[]) {
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  if (!context) {
    return [];
  }
  const sample = '任务编排 AI Workflow 0123456789';
  const size = '72px';
  const baseFamilies = ['monospace', 'sans-serif', 'serif'] as const;
  const baselines = new Map(
    baseFamilies.map((family) => {
      context.font = `${size} ${family}`;
      return [family, context.measureText(sample).width] as const;
    }),
  );
  return normalizeFontCatalogFamilies(
    candidates.filter((family) => {
      const quoted = quoteFontFamily(family);
      if (document.fonts.check(`16px ${quoted}`)) {
        return true;
      }
      return baseFamilies.some((baseFamily) => {
        context.font = `${size} ${quoted}, ${baseFamily}`;
        return context.measureText(sample).width !== baselines.get(baseFamily);
      });
    }),
  );
}

async function queryBrowserLocalFonts() {
  const fontWindow = window as LocalFontWindow;
  if (typeof fontWindow.queryLocalFonts !== 'function') {
    return [];
  }
  try {
    const fonts = await fontWindow.queryLocalFonts();
    return normalizeFontCatalogFamilies(fonts.map((font) => font.family));
  } catch {
    return [];
  }
}

function quoteFontFamily(family: string) {
  return `"${family.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`;
}

void mockWorkflowTemplates;
void toRoundSelectionInput;
void mockBootstrap;
void mockContent;
void mockAgentRegistry;
void mockTaskDetail;
void mockWorkflow;
void mockRoundDetail;
void mockRunDetail;
void mockLogPage;
void mockTaskList;
