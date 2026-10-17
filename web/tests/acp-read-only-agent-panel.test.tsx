/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return {
    ...actual,
    getAcpSession: vi.fn().mockResolvedValue(null),
    respondAcpPermission: vi.fn().mockResolvedValue(null),
  };
});

import { getAcpSession, respondAcpPermission } from '@/api';
import {
  ACPChatDialog,
  createAcpSessionCacheKey,
  markAcpSessionContentHydrated,
  resetAcpResourceCache,
  storeAcpLoadedEventWindow,
  storeAcpSession,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { RightWorkspaceProvider } from '@/components/workspace/right-workspace-context';
import {
  applyConversationEventToBranchSnapshots,
  resetConversationEventRouterSnapshots,
} from '@/lib/conversation-event-router';
import { detachConversationViewport } from './acp/detach-conversation-viewport';
import type { AcpSessionVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function TestProviders({ children }: { children: React.ReactNode }) {
  return React.createElement(
    RightWorkspaceProvider,
    null,
    React.createElement(TooltipProvider, null, children),
  );
}

beforeEach(() => {
  resetConversationEventRouterSnapshots();
  resetAcpResourceCache();
  vi.mocked(getAcpSession).mockReset().mockResolvedValue(null);
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
});

function session(branchId: string, withPermission = false): AcpSessionVm {
  return {
    branchId,
    parentBranchId: branchId === 'root' ? null : 'root',
    readOnly: branchId !== 'root',
    branchExecution: branchId === 'root' ? null : {
      agentExecutionId: branchId,
      parentAgentExecutionId: null,
      executionStatus: withPermission ? 'waiting_permission' : 'completed',
      eventCount: 8,
      toolCallCount: 3,
      readFileCount: 2,
      writtenFileCount: 1,
      hasAttention: withPermission,
      title: 'Agent branch',
      description: 'Agent branch',
      todoEntries: [],
    },
    sessionId: 'session-1',
    title: 'Agent branch',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    provider: 'test',
    status: withPermission ? 'running' : 'completed',
    restored: false,
    events: [],
    eventPage: {
      loadedCount: 0,
      total: 0,
      oldestSeq: null,
      newestSeq: null,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    timelineProjection: { agents: [], todoEntries: [] },
    pendingInteractions: withPermission ? [{
      kind: 'permission',
      interactionId: 'permission-1',
      title: 'Read file',
      toolCallId: 'tool-1',
      raw: { rawInput: { path: 'README.md' } },
      options: [{ optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' }],
    }] : [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
}

async function renderDialog(acpSession: AcpSessionVm, readOnly: boolean) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TestProviders>
        <ACPChatDialog
          session={acpSession}
          projectId="project-1"
          taskId="task-1"
          runId="run-1"
          roundId="round-1"
          nodeId="node-1"
          attemptId="attempt-1"
          branchId={acpSession.branchId}
          readOnly={readOnly}
          showSystemPromptAction={false}
          showRawFramesAction={false}
          usageCompact
        />
      </TestProviders>,
    );
  });
  return { container, root };
}

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('read-only Agent conversation boundary', () => {
  it.each(['running', 'cancelled', 'completed'])('shows unknown Agent status without a summary while parent is %s', async (status) => {
    const rootSession = session('root');
    rootSession.status = status;
    rootSession.events = [{
      id: 'agent-launch-unknown', seq: 1, timestamp: '1Z', kind: 'toolCall',
      sessionId: 'session-1', content: null, title: 'Audit', toolCallId: 'launch-unknown',
      status: 'completed', raw: { _meta: { goldBandConversation: {
        branchId: 'root', launchedAgentExecutionId: 'agent-unknown', toolName: 'Agent',
      } } },
    }];
    const { container, root } = await renderDialog(rootSession, true);
    try {
      const row = container.querySelector('[data-agent-link-branch-id="agent-unknown"]');
      expect(row).not.toBeNull();
      expect(row?.textContent).toContain('状态未知');
      expect(row?.textContent).not.toContain('等待执行');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps one native Agent card through completion, resume and unknown without exposing state rows', async () => {
    const rootSession = session('root');
    rootSession.timelineProjection = {
      agents: [{ ...session('agent-1').branchExecution!, providerStateSeq: 10 }], todoEntries: [],
    };
    rootSession.events = [{
      id: 'native-launch', seq: 1, timestamp: '1Z', kind: 'toolCall',
      sessionId: 'session-1', title: 'Native child', toolCallId: 'native-launch', status: 'completed',
      raw: { _meta: { goldBandConversation: { branchId: 'root', launchedAgentExecutionId: 'agent-1', toolName: 'Agent' } } },
    }, {
      id: 'state-fact', seq: 10, timestamp: '10Z', kind: 'agentState',
      sessionId: 'session-1', content: null, title: 'Hidden native state fact', status: 'completed',
      raw: { source: 'nativeSubagentState' },
    }];
    const { container, root } = await renderDialog(rootSession, true);
    try {
      const card = container.querySelector('[data-agent-link-branch-id="agent-1"]');
      expect(card?.getAttribute('data-agent-link-status')).toBe('completed');
      expect(container.textContent).not.toContain('Hidden native state fact');
      for (const [status, seq] of [['running', 12], ['unknown', 13]] as const) {
        await act(async () => applyConversationEventToBranchSnapshots({
          projectId: 'project-1', taskId: 'task-1', runId: 'run-1', roundId: 'round-1',
          nodeId: 'node-1', attemptId: 'attempt-1', branchId: 'agent-1', timelineGeneration: 1, timelineRevision: seq,
          event: { id: `state-${seq}`, seq, endedSeq: seq, timestamp: `${seq}Z`, kind: 'agentState',
            sessionId: 'session-1', content: null, status, raw: { source: 'nativeSubagentState' } },
        }));
        expect(container.querySelectorAll('[data-agent-link-branch-id="agent-1"]')).toHaveLength(1);
        expect(container.querySelector('[data-agent-link-branch-id="agent-1"]')).toBe(card);
        expect(card?.getAttribute('data-agent-link-status')).toBe(status);
      }
      expect(card?.textContent).toContain('状态未知');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it.each([
    ['completed', 10, 'running', 12, 'running'],
    ['running', 12, 'completed', 10, 'running'],
    ['interrupted', 10, 'running', 10, 'interrupted'],
  ] as const)('orders native branch summary %s/%s against live %s/%s', async (persistedStatus, persistedSeq, liveStatus, liveSeq, expected) => {
    applyConversationEventToBranchSnapshots({
      projectId: 'project-1', taskId: 'task-1', runId: 'run-1', roundId: 'round-1',
      nodeId: 'node-1', attemptId: 'attempt-1', branchId: 'agent-1', timelineGeneration: 1, timelineRevision: liveSeq,
      event: { id: `state-${liveSeq}`, seq: liveSeq, endedSeq: liveSeq, timestamp: `${liveSeq}Z`, kind: 'agentState',
        sessionId: 'session-1', content: null, status: liveStatus, raw: { source: 'nativeSubagentState' } },
    });
    const branch = session('agent-1');
    branch.branchExecution = { ...branch.branchExecution!, executionStatus: persistedStatus, providerStateSeq: persistedSeq };
    const { container, root } = await renderDialog(branch, true);
    try {
      expect(container.querySelector('[data-agent-branch-summary="true"]')
        ?.getAttribute('data-agent-branch-status')).toBe(expected);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('mounts the shared viewport but no composer, stop, continue, or retry controls', async () => {
    const { container, root } = await renderDialog(session('agent-1'), true);
    try {
      expect(container.querySelector('[data-conversation-viewport="true"]')).not.toBeNull();
      expect(container.querySelector('[data-conversation-composer="acp"]')).toBeNull();
      expect(container.querySelector('[data-acp-raw-frames-action]')).toBeNull();
      const summary = container.querySelector('[data-agent-branch-summary="true"]');
      expect(summary).not.toBeNull();
      expect(summary?.getAttribute('data-agent-branch-status')).toBe('completed');
      expect(summary?.getAttribute('data-agent-branch-tool-count')).toBe('3');
      expect(summary?.getAttribute('data-agent-branch-read-file-count')).toBe('2');
      expect(summary?.getAttribute('data-agent-branch-written-file-count')).toBe('1');
      expect(container.textContent).not.toContain('停止');
      expect(container.textContent).not.toContain('继续');
      expect(container.textContent).not.toContain('重试');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not let a stale live running snapshot override an authoritative interruption', async () => {
    applyConversationEventToBranchSnapshots({
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      nodeId: 'node-1',
      attemptId: 'attempt-1',
      branchId: 'agent-1',
      timelineGeneration: 1,
      timelineRevision: 1,
      event: {
        id: 'agent-text-1',
        seq: 1,
        timestamp: '1Z',
        kind: 'textDelta',
        sessionId: 'session-1',
        content: 'working',
        title: null,
        toolCallId: null,
        status: null,
        raw: null,
      },
    });
    const interrupted = session('agent-1');
    interrupted.status = 'interrupted';
    if (interrupted.branchExecution) {
      interrupted.branchExecution.executionStatus = 'interrupted';
    }

    const { container, root } = await renderDialog(interrupted, true);
    try {
      expect(container.querySelector('[data-agent-branch-summary="true"]')
        ?.getAttribute('data-agent-branch-status')).toBe('interrupted');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows an authoritative interrupted status on the parent Agent link', async () => {
    applyConversationEventToBranchSnapshots({
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      nodeId: 'node-1',
      attemptId: 'attempt-1',
      branchId: 'agent-1',
      event: {
        id: 'agent-live-1',
        seq: 2,
        timestamp: '2Z',
        kind: 'textDelta',
        sessionId: 'session-1',
        content: 'working',
        title: null,
        toolCallId: null,
        status: null,
        raw: null,
      },
    });
    const rootSession = session('root');
    rootSession.status = 'cancelled';
    rootSession.events = [{
      id: 'agent-launch-1',
      seq: 1,
      timestamp: '1Z',
      kind: 'toolCall',
      sessionId: 'session-1',
      content: null,
      title: 'Agent branch',
      toolCallId: 'launch-1',
      status: 'completed',
      raw: {
        _meta: {
          goldBandConversation: {
            branchId: 'root',
            launchedAgentExecutionId: 'agent-1',
            toolName: 'Agent',
          },
        },
      },
    }];
    rootSession.timelineProjection = {
      agents: [{
        ...session('agent-1').branchExecution!,
        executionStatus: 'interrupted',
      }],
      todoEntries: [],
    };

    const { container, root } = await renderDialog(rootSession, true);
    try {
      expect(container.querySelector('[data-agent-link-branch-id="agent-1"]')
        ?.getAttribute('data-agent-link-status')).toBe('interrupted');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps a pending Agent permission actionable without mounting the composer', async () => {
    const activeBranch = session('agent-1', true);
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(activeBranch)
      .mockResolvedValueOnce(session('agent-1'));
    const { container, root } = await renderDialog(activeBranch, true);
    try {
      const allow = Array.from(container.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('Allow once'));
      expect(allow).toBeDefined();
      await act(async () => {
        allow?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(respondAcpPermission).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        'permission-1',
        'allow_once',
        expect.anything(),
        undefined,
        undefined,
      );
      expect(getAcpSession).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        expect.objectContaining({ branchId: 'agent-1' }),
        expect.objectContaining({ branchId: 'agent-1' }),
        undefined,
        undefined,
      );
      expect(container.querySelector('[data-conversation-composer="acp"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('offers an explicit return to the latest when the viewport detaches from a historical window', async () => {
    const historical = session('agent-1');
    historical.events = [{
      id: 'agent-history', seq: 1, timestamp: '1Z', kind: 'textDelta',
      sessionId: historical.sessionId, content: 'Earlier Agent message', raw: {},
    }];
    historical.eventPage = {
      ...historical.eventPage, loadedCount: 1, total: 2, oldestSeq: 1, newestSeq: 1, hasNewer: true,
    };
    vi.mocked(getAcpSession).mockResolvedValueOnce(historical);
    const { container, root } = await renderDialog(historical, true);
    try {
      await detachConversationViewport(container);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores a previously loaded Agent Tab without returning to the loading shell', async () => {
    const branchId = 'agent-cached';
    const cacheKey = `${createAcpSessionCacheKey(
      'right-workspace-agent',
      'task-1',
      'run-1',
      'round-1',
      'node-1',
      'attempt-1',
      'project-1',
      null,
      null,
      branchId,
    )}:`;
    const cached = session(branchId);
    storeAcpSession(cacheKey, cached);
    storeAcpLoadedEventWindow(cacheKey, {
      sessionId: cached.sessionId,
      timelineGeneration: 0,
      coveredRevision: 0,
      events: cached.events,
    }, 100);
    markAcpSessionContentHydrated(cacheKey);
    vi.mocked(getAcpSession).mockReturnValueOnce(new Promise(() => {}));
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TestProviders>
            <ACPChatDialog
              session={null}
              projectId="project-1"
              taskId="task-1"
              runId="run-1"
              roundId="round-1"
              nodeId="node-1"
              attemptId="attempt-1"
              branchId={branchId}
              readOnly
              showSystemPromptAction={false}
              showRawFramesAction={false}
              allowEventOnlySessionShell={false}
              usageCompact
              cacheNamespace="right-workspace-agent"
            />
          </TestProviders>,
        );
      });

      expect(container.querySelector('[data-conversation-viewport="true"]')).not.toBeNull();
      expect(container.textContent).not.toContain('加载中');
      expect(getAcpSession).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
