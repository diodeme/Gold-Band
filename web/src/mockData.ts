import type {
  AppBootstrapVm,
  ContentVm,
  PreferencesVm,
  RoundDetailVm,
  RoundSelection,
  RunDetailVm,
  RunSummaryVm,
  TaskDetailVm,
  TaskListVm,
  WorkflowVm,
} from './types';

const preferences: PreferencesVm = { theme: 'dark', language: 'zh-cn' };

const latestRun: RunSummaryVm = {
  id: 'run-003',
  taskId: 'task-001',
  status: 'running',
  outcome: null,
  startedAt: '2026-05-02 15:42',
  updatedAt: '2026-05-02 16:12',
  currentRound: 'round-007',
  currentNode: 'node-03 execute',
  currentAttempt: 'att-2-node03-rev1',
  resumable: true,
  pauseReason: null,
};

const requirement = '重写 Tauri 桌面端的核心窗口管理逻辑，确保 Windows 和 macOS 下的窗口阴影表现一致，并修复多显示器下的 DPI 缩放偏移问题。\n\n目标：重写桌面端窗口与任务编排主界面。\n约束：不引入命令输入或聊天入口；终局状态只来自 canonical state。\n验收：任务列表、工作流、round 详情与设置页均匹配 app 原型。';

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
    { id: 'node-03 execute', label: 'Processing code logic...', nodeType: 'exec', status: 'running', outcome: null, attemptId: 'att-2-node03-rev1', artifactCount: 3, attachmentCount: 2, current: true },
    { id: 'validate', label: 'Verification pending', nodeType: 'verify', status: 'pending', outcome: null, attemptId: null, artifactCount: 0, attachmentCount: 0, current: false },
    { id: 'finalize', label: 'Finalize result', nodeType: 'worker', status: 'pending', outcome: null, attemptId: null, artifactCount: 0, attachmentCount: 0, current: false },
  ],
  edges: [
    { from: 'prepare', to: 'plan', label: 'success' },
    { from: 'plan', to: 'node-03 execute', label: 'success' },
    { from: 'node-03 execute', to: 'validate', label: 'success' },
    { from: 'validate', to: 'finalize', label: 'success' },
  ],
};

const failedAcceptanceGraph = {
  nodes: [
    { id: 'dev', label: '现在我们在测试异常场景，任务会让你输出一个 python 类...', nodeType: 'worker', status: 'completed', outcome: 'success', attemptId: 'attempt-001', artifactCount: 0, attachmentCount: 0, current: false },
    { id: 'accept', label: 'accept', nodeType: 'verify', status: 'completed', outcome: 'failure', attemptId: 'attempt-001', artifactCount: 1, attachmentCount: 0, current: false },
  ],
  edges: [
    { from: 'dev', to: 'accept', label: 'observed' },
  ],
};

const rounds = [
  {
    id: 'round-007',
    runId: 'run-003',
    index: 7,
    status: 'running',
    outcome: null,
    trigger: 'Resume',
    repairLoopsUsed: 1,
    startedAt: '2026-05-02 16:02',
    currentNode: 'node-03 execute',
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
    repairLoopsUsed: 0,
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
    { ...task, id: 'task-003', title: '优化文档结构', displayStatus: 'failed', workflowValid: false, workflowError: 'validation failed', latestRun: { ...latestRun, id: 'run-001', status: 'completed', outcome: 'failure', resumable: false }, resumableRunId: null, artifactCount: 1, attachmentCount: 0 },
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
    maxRepairLoops: 1,
    maxAcceptanceLoops: 1,
    onAcceptanceFailure: 'auto-loop',
  },
  runs: [
    { run: latestRun, rounds },
    { run: { ...latestRun, id: 'run-002', status: 'completed', outcome: 'failure', resumable: false, currentNode: 'validate' }, rounds: [rounds[1]] },
    ...Array.from({ length: 8 }, (_, index) => ({ run: { ...latestRun, id: `run-${String(index + 10).padStart(3, '0')}`, status: 'completed', outcome: index % 2 === 0 ? 'success' : 'failure', resumable: false, currentNode: null }, rounds: rounds.map((round) => ({ ...round, id: `${round.id}-${index}`, runId: `run-${String(index + 10).padStart(3, '0')}`, status: 'completed', outcome: index % 2 === 0 ? 'success' : 'failure' })) })),
  ],
  workflowJson: JSON.stringify(graph, null, 2),
};

export const mockRunDetail: RunDetailVm = {
  run: latestRun,
  rounds,
  events: 'node-03 started\nartifact emitted\nvalidation pending',
  progress: { currentStage: 'node_running' },
};

export function mockRoundDetail(selection?: RoundSelection, route?: { taskId: string; runId: string; roundId: string }): RoundDetailVm {
  const selected = selection?.kind ?? 'round';
  const isFailedAcceptanceRound = route?.runId === 'run-024' && route.roundId === 'round-001';
  const routeRun = isFailedAcceptanceRound ? { ...latestRun, id: 'run-024', status: 'completed', outcome: 'failure', currentRound: 'round-001', currentNode: 'accept', resumable: true } : latestRun;
  const routeRound = isFailedAcceptanceRound ? { ...rounds[0], id: 'round-001', runId: 'run-024', index: 1, status: 'completed', outcome: 'failure', trigger: 'initial', repairLoopsUsed: 0, currentNode: 'accept', artifactCount: 1, attachmentCount: 0 } : rounds[0];
  return {
    run: routeRun,
    round: routeRound,
    graph: isFailedAcceptanceRound ? failedAcceptanceGraph : graph,
    stream: [
      { id: 'requirement', title: 'Requirement', kind: 'requirement', tone: 'accent', content: task.requirementPreview },
      { id: 'round-summary', title: 'Round Summary', kind: 'round', tone: 'success', content: '状态：已完成\n结果：成功\n触发：初始\n修复循环：0\n当前节点：accept' },
      { id: 'node-03', title: 'Selected Node', kind: 'node', tone: 'running', nodeId: 'node-03 execute', content: '状态：运行中\nAttempt：att-2-node03-rev1\n已产出：3 个 artifact / 2 个 attachment' },
      { id: 'artifact-a3', title: 'ARTIFACT A3', kind: 'artifact', tone: 'success', nodeId: 'node-03 execute', attemptId: 'att-2-node03-rev1', name: 'window_manager_v2_core.rs', content: 'window_manager_v2_core.rs updated 2m ago' },
      { id: 'attachment-p2', title: 'ATTACHMENT P2', kind: 'attachment', tone: 'warning', nodeId: 'node-03 execute', attemptId: 'att-2-node03-rev1', name: 'dpi_scaling_logs_win11.txt', content: 'Captured 14m ago' },
      { id: 'event-1', title: 'Provider started generation...', kind: 'event', tone: 'running', nodeId: 'node-03 execute', attemptId: 'att-2-node03-rev1', content: '14:22:05 provider started generation for node-03.' },
      { id: 'log-1', title: 'Runtime Log', kind: 'log', tone: 'muted', nodeId: 'node-03 execute', attemptId: 'att-2-node03-rev1', content: 'node-03 execute stdout: compiling workspace and collecting artifacts.' },
    ],
    detail: mockRoundContent(selection),
  };
}

function mockRoundContent(selection?: RoundSelection): ContentVm {
  if (!selection || selection.kind === 'round') {
    return {
      title: 'Round Summary',
      kind: 'round',
      content: JSON.stringify({ round_id: 'round-007', run_id: 'run-003', status: 'running', current_node: 'node-03 execute' }, null, 2),
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
      content: JSON.stringify({ node_id: selection.nodeId, attempt_id: 'att-2-node03-rev1', status: 'running', artifacts: 3, attachments: 2 }, null, 2),
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
      content: JSON.stringify({ provider: 'claude-code', session_id: 'mock-session-7', node_id: selection.nodeId, attempt_id: selection.attemptId }, null, 2),
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

export const mockContent: ContentVm = {
  title: 'Artifact Preview',
  kind: 'artifact',
  content: 'Mock artifact content',
  metadata: {},
};
