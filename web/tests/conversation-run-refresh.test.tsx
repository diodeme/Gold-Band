/** @vitest-environment jsdom */

import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConversationRunStateUpdatedEventVm } from '@/api/client';
import type { ConversationRunVm, ConversationSessionLeafVm } from '@/types';

const api = vi.hoisted(() => ({
  getRun: vi.fn(),
  retry: vi.fn(),
  rerun: vi.fn(),
  pause: vi.fn(),
  listener: null as ((event: ConversationRunStateUpdatedEventVm) => void) | null,
}));

vi.mock('@/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/api')>(),
  getConversationRun: api.getRun,
  retryCurrentNode: api.retry,
  rerunFailedNode: api.rerun,
  pauseRun: api.pause,
  getConversationSidebarBootstrap: async () => ({
    workspaces: [{ projectId: 'default', workspacePath: 'C:/repo', name: 'Repo' }],
    pinRefs: [], preferences: {}, lastActiveWorkspaceId: 'default',
  }),
  getConversationTaskPage: async () => ({
    projectId: 'default', errors: [], tasks: [{
      projectId: 'default', taskId: 'mock-task', taskUuid: 'task-uuid', title: 'Task',
      runMode: 'workflow', autoTitle: false, runs: [], pinned: false, runHistoryStatus: 'ready',
    }],
  }),
  getConversationRecentTaskPage: async () => ({ tasks: [], errors: [] }),
  getConversationPinnedTaskPage: async () => ({ tasks: [], errors: [] }),
  subscribeConversationRunStateUpdates: async (listener: typeof api.listener) => {
    api.listener = listener;
    return () => { api.listener = null; };
  },
}));

vi.mock('@/components/Shell', () => ({ Shell: ({ children, onConversationPauseRun }: {
  children: ReactNode;
  onConversationPauseRun: (projectId: string, taskId: string, runId: string) => Promise<void>;
}) => <div>
  {children}
  <button data-testid="pause" onClick={() => void onConversationPauseRun('default', 'mock-task', 'run-051')}>Pause</button>
</div> }));
vi.mock('@/components/acp/ACPChatDialog', () => ({ configureAcpResourceCacheSessionCount: vi.fn() }));
vi.mock('@/lib/use-intervention-notifications', () => ({ useInterventionNotifications: vi.fn() }));
vi.mock('@/lib/use-scheduled-notifications', () => ({ useScheduledNotifications: vi.fn() }));
vi.mock('@/components/scheduled-tasks/useScheduledRuntimeSettings', () => ({ prefetchScheduledRuntimeSettings: vi.fn() }));
vi.mock('@/components/settings/useImSettings', () => ({ prefetchImSettings: vi.fn() }));
vi.mock('@/pages/ConversationRunPage', () => ({
  ConversationRunPage: (props: {
    run: ConversationRunVm;
    onNodeRetry: (node: ConversationRunVm['workflowGraph']['nodes'][number]) => Promise<void>;
    onExecutionPlanSaved: () => Promise<void>;
    onSelectSession: (leaf: ConversationSessionLeafVm) => void;
  }) => <div>
    <output data-testid="run-status">{props.run.runStatus}</output>
    <output data-testid="run-id">{props.run.runId}</output>
    <output data-testid="task-uuid">{props.run.taskUuid}</output>
    <output data-testid="graph-status">{props.run.workflowGraph.nodes[0]?.status}</output>
    <output data-testid="graph-retryable">{String(props.run.workflowGraph.nodes[0]?.retryable)}</output>
    <output data-testid="selected-session">{props.run.sessionTree.selectedSessionKey}</output>
    <button data-testid="retry" onClick={() => void props.onNodeRetry(props.run.workflowGraph.nodes[0])}>Retry</button>
    <button data-testid="save" onClick={() => void props.onExecutionPlanSaved()}>Save</button>
    <button data-testid="history" onClick={() => props.onSelectSession(props.run.sessionTree.rounds[0].nodes[0].attempts[0])}>History</button>
  </div>,
}));

import { App } from '@/App';
import { mockErrorBlockedConversationRun } from '@/mockData';

let root: Root | undefined;
let host: HTMLDivElement;

function snapshot(status: string): ConversationRunVm {
  const run = structuredClone(mockErrorBlockedConversationRun);
  run.taskUuid = 'task-uuid';
  run.runStatus = status;
  run.runOutcome = status === 'completed' ? 'success' : null;
  run.pauseReason = status === 'paused' ? 'runtime-abnormal' : null;
  const node = run.sessionTree.rounds[0].nodes[0];
  const original = node.attempts[0];
  const attemptId = status === 'paused' ? 'attempt-002' : 'attempt-003';
  node.attempts = [
    { ...original, attemptId: 'attempt-001', status: 'completed', current: false, lifecycle: null },
    { ...original, attemptId, status, lifecycle: null },
  ];
  run.sessionTree.selectedSessionKey = `round-001/dev/${attemptId}`;
  run.workflowGraph.nodes[0] = {
    ...run.workflowGraph.nodes[0], status, retryable: status === 'paused',
    roundId: 'round-001', nodeId: 'dev', attemptId,
  };
  return run;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function settle() {
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
}

async function click(testId: string) {
  await act(async () => host.querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`)!.click());
}

async function completedEvent() {
  await act(async () => api.listener?.({
    projectId: 'default', taskId: 'mock-task', taskUuid: 'task-uuid', runId: 'run-051',
    roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-003',
    eventKind: 'run-completed', status: 'completed', outcome: 'success',
  }));
  await settle();
}

async function navigate(runId: string) {
  await act(async () => {
    window.history.replaceState(null, '', `/chat/projects/default/tasks/mock-task/runs/${runId}`);
    window.dispatchEvent(new PopStateEvent('popstate'));
  });
  await settle();
}

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} })));
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} });
  api.getRun.mockReset().mockResolvedValue(snapshot('paused'));
  api.retry.mockReset().mockResolvedValue({ operationId: 'retry', locator: {
    projectId: 'default', taskId: 'mock-task', runId: 'run-051',
    roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-003',
  } });
  api.pause.mockReset().mockResolvedValue(undefined);
  api.rerun.mockReset();
  window.history.replaceState(null, '', '/chat/projects/default/tasks/mock-task/runs/run-051');
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root!.render(<App />); });
  await settle();
  expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('paused');
  expect(api.listener).toBeTypeOf('function');
  api.getRun.mockClear();
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('conversation run refresh ordering', () => {
  it('navigates to the successor after rerunning a completed failed node', async () => {
    const source = { ...snapshot('completed'), runId: 'run-terminal', runOutcome: 'failure' };
    source.workflowGraph.nodes[0].retryable = true;
    source.workflowGraph.nodes[0].outcome = 'failure';
    const successor = { ...snapshot('running'), runId: 'run-successor' };
    api.getRun.mockImplementation((_project: string, _task: string, runId: string) => Promise.resolve(runId === 'run-successor' ? successor : source));
    await navigate(source.runId);
    api.rerun.mockResolvedValue({ operationId: 'rerun', locator: { runId: successor.runId } });
    await click('retry');
    await settle();
    expect(api.rerun).toHaveBeenCalledWith(expect.objectContaining({ runId: source.runId, expectedAttemptId: 'attempt-003' }));
    expect(api.retry).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe(successor.runId);
    expect(window.location.pathname).toContain('run-successor');
  });

  it('refreshes a sealed source when its successor Run changes', async () => {
    const sealedSource = {
      ...snapshot('completed'),
      runId: 'run-source',
      runOutcome: 'failure' as const,
      sealed: true,
      rerunSuccessorRunId: 'run-successor',
    };
    sealedSource.workflowGraph.nodes[0].retryable = false;
    const settledSource = {
      ...sealedSource,
      workflowGraph: {
        ...sealedSource.workflowGraph,
        nodes: sealedSource.workflowGraph.nodes.map((node, index) => ({
          ...node,
          retryable: index === 0,
        })),
      },
    };
    api.getRun.mockResolvedValue(sealedSource);
    await navigate(sealedSource.runId);
    expect(host.querySelector('[data-testid="graph-retryable"]')?.textContent).toBe('false');
    api.getRun.mockClear();
    api.getRun.mockResolvedValue(settledSource);
    await act(async () => api.listener?.({
      projectId: 'default', taskId: 'mock-task', taskUuid: 'task-uuid', runId: 'run-successor',
      roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-004',
      eventKind: 'run-completed', status: 'completed', outcome: 'success',
    }));
    await settle();
    expect(api.getRun).toHaveBeenCalledWith(
      'default',
      'mock-task',
      'run-source',
      'round-001/dev/attempt-003',
    );
    expect(host.querySelector('[data-testid="graph-retryable"]')?.textContent).toBe('true');
  });

  it('ignores a rerun acceptance after navigation to another run', async () => {
    const source = { ...snapshot('completed'), runId: 'run-terminal', runOutcome: 'failure' };
    source.workflowGraph.nodes[0].retryable = true;
    api.getRun.mockResolvedValue(source);
    await navigate(source.runId);
    const command = deferred<unknown>();
    api.rerun.mockReturnValue(command.promise);
    await click('retry');
    api.getRun.mockResolvedValue({ ...snapshot('completed'), runId: 'run-other' });
    await navigate('run-other');
    api.getRun.mockClear();
    await act(async () => command.resolve({ operationId: 'rerun', locator: { runId: 'run-successor' } }));
    await settle();
    expect(api.getRun).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe('run-other');
  });
  it.each(['retry', 'pause', 'save'])('keeps the completed run and graph after a delayed %s refresh settles', async (action) => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue(snapshot('completed'));
    await click(action);
    expect(api.getRun).toHaveBeenCalledTimes(1);

    await completedEvent();
    expect(api.getRun).toHaveBeenCalledTimes(1);
    await act(async () => delayed.resolve(snapshot('running')));
    await settle();

    expect(api.getRun).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
    expect(host.querySelector('[data-testid="graph-status"]')?.textContent).toBe('completed');
  });

  it('coalesces retry and save behind an earlier live refresh', async () => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue(snapshot('completed'));
    await completedEvent();
    await click('retry');
    await click('retry');
    await click('save');
    expect(api.getRun).toHaveBeenCalledTimes(1);
    expect(api.retry).toHaveBeenCalledTimes(1);
    expect(api.retry).toHaveBeenCalledWith({
      projectId: 'default', taskId: 'mock-task', runId: 'run-051',
      expectedRoundId: 'round-001', expectedNodeId: 'dev', expectedAttemptId: 'attempt-002',
    });

    await act(async () => delayed.resolve(snapshot('running')));
    await settle();
    expect(api.getRun).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });

  it('refreshes a stale locator once without submitting retry again', async () => {
    api.retry.mockRejectedValue({ code: 'workflow.retry.stale-locator' });
    api.getRun.mockResolvedValue(snapshot('completed'));
    await click('retry');
    await settle();
    expect(api.retry).toHaveBeenCalledTimes(1);
    expect(api.getRun).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });

  it('continues to the latest read when an obsolete read fails', async () => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue(snapshot('completed'));
    await click('retry');
    await completedEvent();
    await act(async () => delayed.reject({ code: 'test.obsolete-read' }));
    await settle();
    expect(api.getRun).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });

  it('can refresh again after the latest read fails without clearing the graph', async () => {
    api.getRun.mockRejectedValueOnce({ code: 'test.latest-read' }).mockResolvedValue(snapshot('completed'));
    await click('retry');
    await settle();
    expect(host.querySelector('[data-testid="graph-status"]')?.textContent).toBe('paused');
    await completedEvent();
    expect(api.getRun).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });

  it('preserves a manual session selection made while retry is refreshing', async () => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue(snapshot('completed'));
    await click('retry');
    await click('history');
    expect(host.querySelector('[data-testid="selected-session"]')?.textContent).toBe('round-001/dev/attempt-001');
    await act(async () => delayed.resolve(snapshot('running')));
    await settle();
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
    expect(host.querySelector('[data-testid="selected-session"]')?.textContent).toBe('round-001/dev/attempt-001');
  });

  it('does not refresh a different run after an earlier retry command is accepted', async () => {
    const command = deferred<unknown>();
    api.retry.mockReturnValue(command.promise);
    await click('retry');
    api.getRun.mockResolvedValue({ ...snapshot('completed'), runId: 'run-052' });
    await navigate('run-052');
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe('run-052');
    api.getRun.mockClear();
    await act(async () => command.resolve({ operationId: 'retry' }));
    await settle();
    expect(api.getRun).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe('run-052');
  });

  it.each(['retry', 'save'])('ignores a delayed %s snapshot when its readable locator is reused', async (action) => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue({
      ...snapshot('completed'), taskUuid: 'replacement-task-uuid',
    });
    await click(action);
    await navigate('run-051');
    await act(async () => delayed.resolve(snapshot('running')));
    await settle();
    expect(host.querySelector('[data-testid="task-uuid"]')?.textContent).toBe('replacement-task-uuid');
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });

  it.each(['retry', 'pause', 'save'])('rejects a %s response for a different canonical task', async (action) => {
    api.getRun.mockResolvedValue({ ...snapshot('completed'), taskUuid: 'replacement-task-uuid' });
    await click(action);
    await settle();
    expect(host.querySelector('[data-testid="task-uuid"]')?.textContent).toBe('task-uuid');
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('paused');
  });

  it('does not apply a saved workflow snapshot to a different run', async () => {
    const delayed = deferred<ConversationRunVm>();
    api.getRun.mockImplementationOnce(() => delayed.promise).mockResolvedValue({ ...snapshot('completed'), runId: 'run-052' });
    await click('save');
    await navigate('run-052');
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe('run-052');
    await act(async () => delayed.resolve(snapshot('running')));
    await settle();
    expect(host.querySelector('[data-testid="run-id"]')?.textContent).toBe('run-052');
    expect(host.querySelector('[data-testid="run-status"]')?.textContent).toBe('completed');
  });
});
