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
  storeAcpSession,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpSessionVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
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
    pendingPermissions: withPermission ? [{
      requestId: 'permission-1',
      title: 'Read file',
      toolCallId: 'tool-1',
      raw: { rawInput: { path: 'README.md' } },
      options: [{ optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' }],
    }] : [],
    pendingElicitations: [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
}

async function renderDialog(acpSession: AcpSessionVm, readOnly: boolean) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
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
      </TooltipProvider>,
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

  it('keeps a pending Agent permission actionable without mounting the composer', async () => {
    const activeBranch = session('agent-1', true);
    vi.mocked(getAcpSession).mockResolvedValueOnce(session('agent-1'));
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

  it('offers an explicit return to the latest semantic window', async () => {
    const historical = session('agent-1');
    historical.eventPage.hasNewer = true;
    const { container, root } = await renderDialog(historical, true);
    try {
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
    )}:::${branchId}:`;
    storeAcpSession(cacheKey, session(branchId));
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
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
          </TooltipProvider>,
        );
      });

      expect(container.querySelector('[data-conversation-viewport="true"]')).not.toBeNull();
      expect(container.textContent).not.toContain('加载中');
      expect(getAcpSession).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
