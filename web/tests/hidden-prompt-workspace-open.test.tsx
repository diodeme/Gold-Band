/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import {
  createConversationWorkspaceScope,
  hiddenPromptSectionWorkspaceResourceKey,
  RightWorkspaceProvider,
  useRightWorkspace,
  type AgentTranscriptLocator,
} from '@/components/workspace/right-workspace-context';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const branchLocator: AgentTranscriptLocator = {
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId: 'root',
};

function WorkspaceProbe() {
  const workspace = useRightWorkspace();
  return (
    <output
      data-tab-count={workspace.tabs.length}
      data-active-tab-key={workspace.activeTabKey ?? ''}
      data-requested-open={String(workspace.requestedOpen)}
    />
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('hidden prompt workspace navigation', () => {
  it('opens the canonical hidden section resource from the message link', async () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    Object.defineProperty(Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    });
    const event: AcpUiEventVm = {
      id: 'prompt-1',
      seq: 42,
      timestamp: '2026-08-17T10:00:00Z',
      kind: 'userTextDelta',
      content: '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">runtime</hidden>\n# Requirement',
    };
    const projection = buildAcpTimelineProjection([event], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider scope={createConversationWorkspaceScope({
            projectId: 'project-1',
            taskId: 'task-1',
            runId: 'run-1',
          })}>
            <ACPMessageList
              timeline={projection.timeline}
              sessionStatus="completed"
              sending={false}
              branchLocator={branchLocator}
            />
            <WorkspaceProbe />
          </RightWorkspaceProvider>,
        );
      });

      const link = container.querySelector<HTMLButtonElement>('[data-hidden-prompt-link="true"]');
      await act(async () => link?.click());

      const expectedKey = hiddenPromptSectionWorkspaceResourceKey({
        ...branchLocator,
        eventId: 'prompt-1',
        eventSeq: 42,
        partIndex: 0,
      });
      const probe = container.querySelector('output');
      expect(probe?.getAttribute('data-tab-count')).toBe('1');
      expect(probe?.getAttribute('data-active-tab-key')).toBe(expectedKey);
      expect(probe?.getAttribute('data-requested-open')).toBe('true');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
