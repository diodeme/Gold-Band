import type {
  AgentRegistryVm,
  AppBootstrapVm,
  ContentVm,
  LogPageVm,
  LogQueryInput,
  NodeDetailVm,
  PreferencesVm,
  ProfileListVm,
  UpdateStatusVm,
  UpdaterSettingsVm,
  RoundDetailVm,
  RoundSelection,
  RunDetailVm,
  RunSummaryVm,
  TaskDetailVm,
  TaskListVm,
  WorkflowDsl,
  WorkflowTemplateStore,
  WorkflowVm,
} from './types';

const preferences: PreferencesVm = { theme: 'system', language: 'zh-cn', font: 'app-default' };
export const mockAppInfo = {
  channel: 'default',
  appName: 'Gold Band',
  appKey: 'gold-band',
  configDirName: '.gold-band',
};

export const mockUpdaterSettings: UpdaterSettingsVm = {
  channel: 'default',
  builtInUrl: 'https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json',
  overrideUrl: null,
  effectiveUrl: 'https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json',
  pollIntervalMinutes: 240,
};
export const mockUpdateStatus: UpdateStatusVm = {
  status: 'idle',
  checkedAt: null,
  update: null,
  error: null,
  background: false,
};
let browserUpdaterSettings = { ...mockUpdaterSettings };
let browserUpdateStatus = { ...mockUpdateStatus };
const profileTimestamp = localTimestamp();

function localTimestamp(date = new Date()) {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

const latestRun: RunSummaryVm = {
  id: 'run-003',
  taskId: 'task-001',
  status: 'running',
  outcome: null,
  startedAt: '2026-05-02 15:42',
  updatedAt: '2026-05-02 16:12',
  currentRound: 'round-007',
  currentNode: 'test',
  currentAttempt: 'att-test-001',
  resumable: true,
  pauseReason: null,
};

const requirement = '重写 Tauri 桌面端的核心窗口管理逻辑，确保 Windows 和 macOS 下的窗口阴影表现一致，并修复多显示器下的 DPI 缩放偏移问题。\n\n目标：重写桌面端窗口与任务编排主界面。\n约束：不引入命令输入或聊天入口；终局状态只来自 canonical state。\n验收：任务列表、工作流、round 详情与设置页均匹配 app 原型。';

const defaultWorkflow: WorkflowDsl = {
  version: '0.1',
  id: 'task-workflow',
  entry: 'plan',
  control: {},
  nodes: [
    { type: 'worker', id: 'plan', provider: 'claude-acp', profile: 'pf-m9jw0wq1-a7k3d2s1', goal: 'Analyze the imported requirement and produce an implementation plan.' },
    { type: 'worker', id: 'dev', provider: 'claude-acp', profile: 'pf-m9jw0wq2-q8s6k4n0', goal: 'Implement the requirement in the workspace.' },
    { type: 'worker', id: 'review', provider: 'claude-acp', profile: 'pf-m9jw0wq3-r2x9p7m5', goal: 'Review the implementation and return JSON with result and reason fields.', output: { kind: 'json', artifact: 'review-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'test', provider: 'claude-acp', profile: 'pf-m9jw0wq4-t3y8r1c6', goal: 'Run or describe verification and return JSON with result and reason fields.', output: { kind: 'json', artifact: 'test-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'accept', provider: 'claude-acp', profile: 'pf-m9jw0wq5-u4z7s2d7', goal: 'Validate acceptance and return JSON with result and reason fields.', output: { kind: 'json', artifact: 'accept-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'cleanup', provider: 'claude-acp', profile: 'pf-m9jw0wq6-v5a8t3e8', goal: 'Clean up resources, finalize handoff notes, and close the task after acceptance succeeds.' },
  ],
  edges: [
    { from: 'plan', to: 'dev', on: 'success' },
    { from: 'dev', to: 'review', on: 'success' },
    { from: 'review', to: 'test', on: 'success' },
    { from: 'review', to: 'dev', on: 'failure', session: 'continue' },
    { from: 'test', to: 'accept', on: 'success' },
    { from: 'test', to: 'dev', on: 'failure', session: 'continue' },
    { from: 'accept', to: 'cleanup', on: 'success' },
    { from: 'cleanup', to: '$end', on: 'success' },
    { from: 'accept', to: '$new-round', on: 'failure' },
  ],
};

export const mockWorkflowTemplates: WorkflowTemplateStore = {
  version: '0.1',
  lastUsedTemplateId: 'default',
  lastCreatedWorkflow: null,
  templates: [{ id: 'default', name: '默认工作流', workflow: defaultWorkflow, createdAt: '2026-05-17T00:00:00Z', updatedAt: '2026-05-17T00:00:00Z' }],
};

export const mockProfileList: ProfileListVm = {
  profiles: [
    { id: 'pf-m9jw0wq1-a7k3d2s1', name: '方案', summary: '方案角色，用于需求分析和实施方案设计。', content: '## 方案角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/方案-pf-m9jw0wq1-a7k3d2s1.md' },
    { id: 'pf-m9jw0wq2-q8s6k4n0', name: '开发', summary: '开发角色，用于实现需求并维护代码质量。', content: '## 开发角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/开发-pf-m9jw0wq2-q8s6k4n0.md' },
    { id: 'pf-m9jw0wq3-r2x9p7m5', name: '审查', summary: '审查角色，用于检查实现质量、风险和一致性。', content: '## 审查角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/审查-pf-m9jw0wq3-r2x9p7m5.md' },
    { id: 'pf-m9jw0wq4-t3y8r1c6', name: '测试', summary: '测试角色，用于执行验证并反馈质量结果。', content: '## 测试角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/测试-pf-m9jw0wq4-t3y8r1c6.md' },
    { id: 'pf-m9jw0wq5-u4z7s2d7', name: '验收', summary: '验收角色，用于对照需求判断交付是否满足目标。', content: '## 验收角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/验收-pf-m9jw0wq5-u4z7s2d7.md' },
    { id: 'pf-m9jw0wq6-v5a8t3e8', name: '清理', summary: '清理角色，用于验收成功后的资源释放、收尾和环境清理。', content: '## 清理角色\n\n后续补充完整角色说明。', scope: 'user', createdAt: profileTimestamp, updatedAt: profileTimestamp, path: '~/.gold-band/context/profiles/清理-pf-m9jw0wq6-v5a8t3e8.md' },
  ],
};

const task = {
  id: 'task-001',
  title: 'Tauri 桌面端重写',
  description: 'Refactor legacy electron modules to native Rust/Tauri framework.',
  requirement,
  requirementPreview: '重写 Tauri 桌面端的核心窗口管理逻辑，确保 Windows 和 macOS 下的窗口阴影表现一致，并修复多显示器下的 DPI 缩放偏移问题。',
  displayStatus: 'running',
  workflowExists: true,
  workflowValid: true,
  workflowError: null,
  latestRun,
  resumableRunId: 'run-003',
  artifactCount: 8,
  attachmentCount: 3,
};

const graph = {
  nodes: [
    { id: 'prepare', label: 'Initialization complete', nodeType: 'worker', status: 'success', outcome: 'success', attemptId: 'att-1', artifactCount: 1, attachmentCount: 0, current: false },
    { id: 'plan', label: 'Workflow strategy defined', nodeType: 'worker', status: 'success', outcome: 'success', attemptId: 'att-1', artifactCount: 3, attachmentCount: 0, current: false },
    { id: 'test', label: 'Checking output result...', nodeType: 'worker', status: 'running', outcome: null, attemptId: 'att-test-001', artifactCount: 3, attachmentCount: 2, current: true },
    { id: 'validate', label: 'Acceptance pending', nodeType: 'worker', status: 'pending', outcome: null, attemptId: null, artifactCount: 0, attachmentCount: 0, current: false },
    { id: 'finalize', label: 'Finalize result', nodeType: 'worker', status: 'pending', outcome: null, attemptId: null, artifactCount: 0, attachmentCount: 0, current: false },
  ],
  edges: [
    { from: 'prepare', to: 'plan', label: 'success' },
    { from: 'plan', to: 'test', label: 'success' },
    { from: 'test', to: 'validate', label: 'success' },
    { from: 'validate', to: 'finalize', label: 'success' },
  ],
};

const failedAcceptanceGraph = {
  nodes: [
    { id: 'dev', label: '现在我们在测试异常场景，任务会让你输出一个 python 类...', nodeType: 'worker', status: 'completed', outcome: 'success', attemptId: 'attempt-001', artifactCount: 0, attachmentCount: 0, current: false },
    { id: 'accept', label: 'accept', nodeType: 'worker', status: 'completed', outcome: 'failure', attemptId: 'attempt-001', artifactCount: 1, attachmentCount: 0, current: false },
  ],
  edges: [
    { from: 'dev', to: 'accept', label: 'observed' },
  ],
};

const errorBlockedGraph = {
  nodes: [
    { id: 'dev', label: 'dev', nodeType: 'worker', status: 'paused', outcome: null, attemptId: 'attempt-001', artifactCount: 0, attachmentCount: 0, current: true },
    { id: 'accept', label: 'accept', nodeType: 'worker', status: 'pending', outcome: null, attemptId: null, artifactCount: 0, attachmentCount: 0, current: false },
  ],
  edges: [
    { from: 'dev', to: 'accept', label: 'success' },
  ],
};

const mockNodeDetail: NodeDetailVm = {
  id: 'test',
  nodeId: 'test',
  sequence: 3,
  label: 'Checking output result...',
  nodeType: 'worker',
  provider: 'claude-acp',
  providerDisplayName: 'Claude',
  status: 'running',
  outcome: null,
  attemptId: 'att-test-001',
  current: true,
  startedAt: '2026-05-02 16:08',
  finishedAt: null,
  artifactCount: 3,
  attachmentCount: 2,
  hasProgressEvents: true,
  hasRawStream: true,
  hasWorkerRef: true,
  manualCheckEnabled: false,
  manualCheckPending: false,
  acpSession: {
    sessionId: 'acp-session-7f3',
    provider: 'claude-acp',
    adapterId: 'claude-agent-acp',
    adapterDisplayName: 'Claude ACP',
    cwd: 'D:\\Projects\\code\\ai\\Gold-Band',
    status: 'running',
    sessionElapsedSeconds: 240,
    restored: true,
    stopReason: null,
    systemPromptAppend: '你正在 Gold Band runtime 中执行一个工作流节点。\n\n当前是：\n- Project: mock-project\n- Node: dev\n\nGold Band 文件规则：\n- 当前节点所需上下文已在本 prompt 中给出。',
    diagnostics: { rawFrameCount: 18, eventCount: 7, errorCount: 0, lastError: null, lastErrorTimestamp: null },
    eventPage: { loadedCount: 5, total: 7, oldestSeq: 1, newestSeq: 5, hasOlder: false, hasNewer: false },
    pendingPermissions: [
      {
        requestId: 'perm-001',
        title: '允许写入窗口管理文件',
        toolCallId: 'tool-2',
        options: [
          { optionId: 'allow-once', name: '允许一次', kind: 'allow_once' },
          { optionId: 'reject-once', name: '拒绝', kind: 'reject_once' },
        ],
        raw: {},
      },
    ],
    events: [
      { id: 'e1', seq: 1, timestamp: '2026-05-02 16:08', kind: 'textDelta', content: '我会先检查窗口管理相关文件。', sessionId: 'acp-session-7f3', raw: {} },
      { id: 'e2', seq: 2, timestamp: '2026-05-02 16:09', kind: 'thoughtDelta', content: '需要确认 DPI 缩放和阴影配置是否共享状态。', sessionId: 'acp-session-7f3', raw: {} },
      { id: 'e3', seq: 3, timestamp: '2026-05-02 16:10', kind: 'toolCall', title: 'Read window manager', toolCallId: 'tool-1', status: 'completed', sessionId: 'acp-session-7f3', raw: { toolCallId: 'tool-1', title: 'Read window manager', status: 'completed' } },
      { id: 'e4', seq: 4, timestamp: '2026-05-02 16:11', kind: 'plan', sessionId: 'acp-session-7f3', raw: { entries: [{ content: '重构窗口状态', status: 'completed' }, { content: '修正 DPI 偏移', status: 'in_progress' }] } },
      { id: 'e5', seq: 5, timestamp: '2026-05-02 16:12', kind: 'permissionRequest', title: '允许写入窗口管理文件', toolCallId: 'tool-2', status: 'pending', sessionId: 'acp-session-7f3', raw: { options: [{ optionId: 'allow-once', name: '允许一次', kind: 'allow_once' }, { optionId: 'reject-once', name: '拒绝', kind: 'reject_once' }] } },
    ],
  },
  artifacts: [
    { kind: 'artifact', name: 'window_manager_v2_core.rs', title: 'window_manager_v2_core.rs', tone: 'accent', preview: 'canonical artifact', nodeId: 'test', attemptId: 'att-test-001' },
    { kind: 'artifact', name: 'layout_patch.json', title: 'layout_patch.json', tone: 'accent', preview: 'layout patch', nodeId: 'test', attemptId: 'att-test-001' },
  ],
  attachments: [
    { kind: 'attachment', name: 'dpi_scaling_logs_win11.txt', title: 'dpi_scaling_logs_win11.txt', tone: 'neutral', preview: 'provider attachment', nodeId: 'test', attemptId: 'att-test-001' },
  ],
};

const errorBlockedNodeDetail: NodeDetailVm = {
  ...mockNodeDetail,
  id: 'dev',
  nodeId: 'dev',
  sequence: 1,
  label: 'dev',
  nodeType: 'worker',
  status: 'paused',
  outcome: null,
  attemptId: 'attempt-001',
  current: true,
  artifactCount: 0,
  attachmentCount: 0,
  artifacts: [],
  attachments: [],
  acpSession: mockNodeDetail.acpSession ? {
    ...mockNodeDetail.acpSession,
    status: 'failed',
    sessionElapsedSeconds: 62,
    diagnostics: { rawFrameCount: 5, eventCount: 2, errorCount: 1, lastError: 'ACP prompt failed: adapter returned malformed response', lastErrorTimestamp: '2026-05-15 10:02' },
    eventPage: { loadedCount: 3, total: 3, oldestSeq: 1, newestSeq: 3, hasOlder: false, hasNewer: false },
    pendingPermissions: [],
    events: [
      { id: 'e1', seq: 1, timestamp: '2026-05-15 10:01', kind: 'userTextDelta', content: '初始需求 prompt', sessionId: 'acp-session-7f3', raw: { source: 'goldBandPrompt', synthetic: true } },
      { id: 'acp-diagnostic-error-1', seq: 2, timestamp: '2026-05-15 10:02', kind: 'runtimeError', content: 'ACP prompt failed: adapter returned malformed response', status: 'failed', raw: { source: 'acpDiagnostic', level: 'error' } },
      { id: 'gold-band-user-prompt-3', seq: 3, timestamp: '2026-05-15 10:03', kind: 'userTextDelta', content: '继续', status: 'completed', sessionId: 'acp-session-7f3', raw: { source: 'goldBandPrompt', synthetic: true } },
    ],
  } : null,
};

const rounds = [
  {
    id: 'round-007',
    runId: 'run-003',
    index: 7,
    status: 'running',
    outcome: null,
    trigger: 'Resume',
    startedAt: '2026-05-02 16:02',
    currentNode: 'test',
    artifactCount: 5,
    attachmentCount: 2,
  },
  {
    id: 'round-006',
    runId: 'run-003',
    index: 6,
    status: 'completed',
    outcome: 'success',
    trigger: 'manual',
    startedAt: '2026-05-02 15:54',
    currentNode: 'validate',
    artifactCount: 3,
    attachmentCount: 1,
  },
];

export const mockBootstrap: AppBootstrapVm = {
  repoRoot: 'D:\\Projects\\code\\ai\\Gold-Band',
  recentWorkspaces: ['D:\\Projects\\code\\ai\\Gold-Band'],
  preferences,
  updaterSettings: browserUpdaterSettings,
  updateStatus: browserUpdateStatus,
  clientVersion: '',
  appInfo: mockAppInfo,
};

export const mockAgentRegistry: AgentRegistryVm = {
  agents: [
    {
      agentType: 'claude-acp',
      displayName: 'Claude',
      command: 'npx',
      args: ['-y', '@agentclientprotocol/claude-agent-acp@0.37.0'],
      env: [{ key: 'ANTHROPIC_API_KEY', value: '***' }],
      iconKey: 'claude',
      supported: true,
      diagnostic: {
        status: 'healthy',
        available: true,
        reason: null,
        checkedAt: '2026-05-16 10:42:00',
      },
      supportedModes: [
        { id: 'ask', name: 'Ask' },
        { id: 'bypass', name: 'Bypass' },
        { id: 'allow-edit', name: 'Allow Edit' },
      ],
    },
  ],
  supportedTypes: [
    { agentType: 'claude-acp', label: 'Claude', iconKey: 'claude', supported: true, configured: true, defaultDisplayName: 'Claude', defaultCommand: 'npx', defaultArgs: ['-y', '@agentclientprotocol/claude-agent-acp@0.37.0'], defaultEnv: [] },
    { agentType: 'codex-acp', label: 'Codex', iconKey: 'codex', supported: true, configured: false, defaultDisplayName: 'Codex', defaultCommand: 'npx', defaultArgs: ['-y', '@zed-industries/codex-acp@0.14.0'], defaultEnv: [] },
    { agentType: 'cursor', label: 'Cursor', iconKey: 'cursor', supported: true, configured: false, defaultDisplayName: 'Cursor', defaultCommand: '.\\dist-package\\cursor-agent.cmd', defaultArgs: ['acp'], defaultEnv: [] },
    { agentType: 'gemini', label: 'Gemini', iconKey: 'gemini', supported: true, configured: false, defaultDisplayName: 'Gemini', defaultCommand: 'npx', defaultArgs: ['-y', '@google/gemini-cli@0.43.0', '--acp'], defaultEnv: [] },
    { agentType: 'opencode', label: 'OpenCode', iconKey: 'opencode', supported: true, configured: false, defaultDisplayName: 'OpenCode', defaultCommand: '.\\opencode.exe', defaultArgs: ['acp'], defaultEnv: [] },
  ],
};

export const mockTaskList: TaskListVm = {
  cards: [
    { key: 'all', label: '全部任务', value: 14, tone: 'accent' },
    { key: 'running', label: '运行中', value: 1, tone: 'running' },
    { key: 'resumable', label: '可恢复', value: 1, tone: 'resumable' },
    { key: 'failed', label: '校验失败', value: 1, tone: 'failed' },
    { key: 'invalid', label: '配置异常', value: 1, tone: 'muted' },
  ],
  tasks: [
    task,
    { ...task, id: 'task-002', title: '修复 provider 输出', displayStatus: 'resumable', latestRun: { ...latestRun, id: 'run-002', status: 'paused', outcome: 'failure', resumable: true }, resumableRunId: 'run-002', artifactCount: 3, attachmentCount: 1 },
    { ...task, id: 'task-003', title: '优化文档结构', displayStatus: 'failed', workflowValid: false, workflowError: { code: 'workflow.invalid', params: {} }, latestRun: { ...latestRun, id: 'run-001', status: 'completed', outcome: 'failure', resumable: false }, resumableRunId: null, artifactCount: 1, attachmentCount: 0 },
    { ...task, id: 'task-004', title: '新增观测索引', requirement: '在web目录下输出一个python类，输出hello-world', requirementPreview: '在web目录下输出一个python类，输出hello-world', displayStatus: 'missing-workflow', workflowExists: false, workflowValid: false, latestRun: null, resumableRunId: null, artifactCount: 0, attachmentCount: 0 },
    ...Array.from({ length: 10 }, (_, index) => ({
      ...task,
      id: `task-${String(index + 5).padStart(3, '0')}`,
      title: `分页验证任务 ${index + 5}`,
      displayStatus: index % 3 === 0 ? 'completed' : index % 3 === 1 ? 'ready' : 'running',
      latestRun: index % 3 === 1 ? null : { ...latestRun, id: `run-${String(index + 10).padStart(3, '0')}`, status: index % 3 === 2 ? 'running' : 'completed', outcome: index % 3 === 2 ? null : 'success', resumable: false },
      resumableRunId: null,
      artifactCount: index + 1,
      attachmentCount: index % 4,
    })),
  ],
};

export const mockTaskDetail: TaskDetailVm = {
  task,
  requirement,
  runs: [latestRun, { ...latestRun, id: 'run-002', status: 'completed', outcome: 'failure', resumable: false, currentRound: 'round-004' }],
};

export const mockWorkflow: WorkflowVm = {
  task,
  graph,
  control: {
    maxAttempts: 3,
    maxRounds: 2,
  },
  runs: [
    { run: latestRun, rounds },
    { run: { ...latestRun, id: 'run-002', status: 'completed', outcome: 'failure', resumable: false, currentNode: 'validate' }, rounds: [rounds[1]] },
    ...Array.from({ length: 8 }, (_, index) => ({ run: { ...latestRun, id: `run-${String(index + 10).padStart(3, '0')}`, status: 'completed', outcome: index % 2 === 0 ? 'success' : 'failure', resumable: false, currentNode: null }, rounds: rounds.map((round) => ({ ...round, id: `${round.id}-${index}`, runId: `run-${String(index + 10).padStart(3, '0')}`, status: 'completed', outcome: index % 2 === 0 ? 'success' : 'failure' })) })),
  ],
  workflowJson: JSON.stringify(defaultWorkflow, null, 2),
};

export const mockRunDetail: RunDetailVm = {
  run: latestRun,
  rounds,
  events: 'node-03 started\nartifact emitted\nacceptance pending',
  progress: { currentStage: 'node_running' },
};

export function mockRoundDetail(selection?: RoundSelection, route?: { taskId: string; runId: string; roundId: string }): RoundDetailVm {
  const isFailedAcceptanceRound = route?.runId === 'run-024' && route.roundId === 'round-001';
  const isErrorBlockedRound = route?.runId === 'run-051' && route.roundId === 'round-001';
  const routeRun = isErrorBlockedRound
    ? { ...latestRun, id: 'run-051', status: 'paused', outcome: null, currentRound: 'round-001', currentNode: 'dev', currentAttempt: 'attempt-001', resumable: true, pauseReason: 'error-blocked' }
    : isFailedAcceptanceRound
      ? { ...latestRun, id: 'run-024', status: 'completed', outcome: 'failure', currentRound: 'round-001', currentNode: 'accept', resumable: true }
      : latestRun;
  const routeRound = isErrorBlockedRound
    ? { ...rounds[0], id: 'round-001', runId: 'run-051', index: 1, status: 'paused', outcome: null, trigger: 'initial', currentNode: 'dev', artifactCount: 0, attachmentCount: 0 }
    : isFailedAcceptanceRound
      ? { ...rounds[0], id: 'round-001', runId: 'run-024', index: 1, status: 'completed', outcome: 'failure', trigger: 'initial', currentNode: 'accept', artifactCount: 1, attachmentCount: 0 }
      : rounds[0];
  const selectedNodeDetail = selection?.kind === 'node' || selection?.kind === 'artifact' || selection?.kind === 'attachment' || selection?.kind === 'worker-ref' || selection?.kind === 'log'
    ? isErrorBlockedRound
      ? { ...errorBlockedNodeDetail, nodeId: selection.nodeId ?? errorBlockedNodeDetail.nodeId }
      : { ...mockNodeDetail, nodeId: selection.nodeId ?? mockNodeDetail.nodeId }
    : null;
  return {
    run: routeRun,
    round: routeRound,
    graph: isErrorBlockedRound ? errorBlockedGraph : isFailedAcceptanceRound ? failedAcceptanceGraph : graph,
    control: mockWorkflow.control,
    requirement,
    selectedNodeDetail,
  };
}

function mockRoundContent(selection?: RoundSelection): ContentVm {
  if (!selection || selection.kind === 'round') {
    return {
      title: 'Round Summary',
      kind: 'round',
      content: JSON.stringify({ round_id: 'round-007', run_id: 'run-003', status: 'running', current_node: 'test' }, null, 2),
      metadata: { source: 'mock-round' },
    };
  }
  if (selection.kind === 'requirement') {
    return {
      title: 'Requirement',
      kind: 'requirement',
      content: task.requirementPreview,
      metadata: { source: 'mock-requirement' },
    };
  }
  if (selection.kind === 'node') {
    return {
      title: selection.nodeId,
      kind: 'node',
      content: JSON.stringify({ node_id: selection.nodeId, attempt_id: 'att-test-001', status: 'running', artifacts: 3, attachments: 2 }, null, 2),
      metadata: { source: 'mock-node' },
    };
  }
  if (selection.kind === 'artifact') {
    return {
      title: selection.name,
      kind: 'artifact',
      content: JSON.stringify({ file: selection.name, node_id: selection.nodeId, attempt_id: selection.attemptId, preview: 'mock canonical artifact content' }, null, 2),
      metadata: { nodeId: selection.nodeId, attemptId: selection.attemptId },
    };
  }
  if (selection.kind === 'attachment') {
    return {
      title: selection.name,
      kind: 'attachment',
      content: JSON.stringify({ file: selection.name, node_id: selection.nodeId, attempt_id: selection.attemptId, preview: 'mock provider attachment content' }, null, 2),
      metadata: { nodeId: selection.nodeId, attemptId: selection.attemptId },
    };
  }
  if (selection.kind === 'worker-ref') {
    return {
      title: `Worker Ref ${selection.nodeId}`,
      kind: 'worker-ref',
      content: JSON.stringify({ provider: 'claude-acp', session_id: 'mock-session-7', node_id: selection.nodeId, attempt_id: selection.attemptId }, null, 2),
      metadata: { nodeId: selection.nodeId, attemptId: selection.attemptId },
    };
  }
  return {
    title: selection.kind === 'log' ? 'Runtime Log' : 'Run Events',
    kind: selection.kind,
    content: JSON.stringify({ id: selection.id, message: selection.kind === 'log' ? 'mock runtime log detail' : 'mock run event detail' }, null, 2),
    metadata: { id: selection.id },
  };
}

export function mockLogPage(query: LogQueryInput): LogPageVm {
  const page = query.page ?? 0;
  const pageSize = query.pageSize ?? 50;
  const total = 126;
  const source = query.source ?? 'system';
  const start = page * pageSize;
  const end = Math.min(total, start + pageSize);
  return {
    page,
    pageSize,
    total,
    hasPrevious: page > 0,
    hasNext: end < total,
    tier: 'hot',
    hotLimit: query.hotLimit ?? 1000,
    archiveRetentionDays: 30,
    items: Array.from({ length: Math.max(0, end - start) }, (_, offset) => {
      const index = start + offset + 1;
      return {
        id: `${source}-${index}`,
        timestamp: `2026-05-11 10:${String(index % 60).padStart(2, '0')}`,
        entryType: source === 'raw-stream' ? 'stdout' : index % 3 === 0 ? 'node_started' : 'provider_event',
        level: source === 'raw-stream' ? 'stdout' : null,
        nodeId: query.scope.nodeId ?? 'test',
        attemptId: query.scope.attemptId ?? 'att-test-001',
        stage: index % 2 === 0 ? 'calling-provider' : 'streaming',
        summary: source === 'raw-stream' ? `raw stream envelope ${index}` : `structured runtime event ${index}`,
        source,
        raw: { index, source },
      };
    }),
  };
}

export const mockContent: ContentVm = {
  title: 'Artifact Preview',
  kind: 'artifact',
  content: 'Mock artifact content',
  metadata: {},
};
