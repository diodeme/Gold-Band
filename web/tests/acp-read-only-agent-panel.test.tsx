/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, respondAcpPermission: vi.fn().mockResolvedValue(null) };
});

import { respondAcpPermission } from '@/api';
import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
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
      expect(container.textContent).not.toContain('停止');
      expect(container.textContent).not.toContain('继续');
      expect(container.textContent).not.toContain('重试');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps a pending Agent permission actionable without mounting the composer', async () => {
    const { container, root } = await renderDialog(session('agent-1', true), true);
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
      expect(container.querySelector('[data-conversation-composer="acp"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
