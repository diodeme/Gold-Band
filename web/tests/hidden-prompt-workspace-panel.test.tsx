/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, getAcpSession: vi.fn() };
});

vi.mock('@/components/acp/ACPChatDialog', () => ({
  RawFrameViewer: () => null,
  SystemPromptPanel: (props: {
    prompt?: string | null;
    documentKey?: string;
    resourceKind?: string;
  }) => (
    <output
      data-testid="read-only-prompt-panel"
      data-document-key={props.documentKey}
      data-resource-kind={props.resourceKind}
    >
      {props.prompt}
    </output>
  ),
}));

import { getAcpSession } from '@/api';
import { ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';
import {
  createHiddenPromptSectionWorkspaceResource,
  type AcpAttemptWorkspaceLocator,
} from '@/components/workspace/right-workspace-context';
import type { AcpSessionVm, ConversationRunVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const attemptLocator: AcpAttemptWorkspaceLocator = {
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId: 'agent-1',
};

afterEach(() => {
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe('hidden prompt workspace panel', () => {
  it('loads one authoritative semantic block and forwards only the located section to the shared read-only panel', async () => {
    vi.mocked(getAcpSession).mockResolvedValue({
      events: [{
        id: 'prompt-1',
        seq: 41,
        endedSeq: 42,
        timestamp: '2026-08-17T10:00:00Z',
        kind: 'userTextDelta',
        content: [
          '<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">system</hidden>',
          '<hidden data-gold-band-hidden="true" title="Gold Band runtime context"># Runtime\ncontext</hidden>',
          '# Requirement',
        ].join('\n'),
      }],
    } as unknown as AcpSessionVm);
    const resource = createHiddenPromptSectionWorkspaceResource({
      scopeKey: 'conversation:project-1:task-1:run-1',
      title: 'Hidden runtime context',
      locator: attemptLocator,
      eventId: 'prompt-1',
      eventSeq: 42,
      partIndex: 2,
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationRunWorkspaceResourcePanel
            resource={resource}
            run={{} as ConversationRunVm}
            agentRegistry={null}
          />,
        );
        await Promise.resolve();
      });

      expect(getAcpSession).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        { branchId: 'agent-1', afterSeq: 41, eventLimit: 1, pageSize: 1 },
        null,
        undefined,
        undefined,
      );
      const panel = container.querySelector('[data-testid="read-only-prompt-panel"]');
      expect(panel?.textContent).toBe('# Runtime\ncontext');
      expect(panel?.getAttribute('data-document-key')).toBe(resource.key);
      expect(panel?.getAttribute('data-resource-kind')).toBe('hidden-prompt-section');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
