/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/components/acp/ACPChatDialog', () => ({
  ACPChatDialog: (props: Record<string, unknown>) => (
    <output
      data-branch-id={String(props.branchId)}
      data-read-only={String(props.readOnly)}
      data-show-system-prompt={String(props.showSystemPromptAction)}
      data-show-raw-frames={String(props.showRawFramesAction)}
      data-usage-compact={String(props.usageCompact)}
    />
  ),
}));

import { AgentConversationPanel } from '@/components/workspace/AgentConversationPanel';
import type { AgentTranscriptResource } from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

describe('AgentConversationPanel', () => {
  it('binds the shared conversation renderer to the read-only Agent boundary', async () => {
    const resource: AgentTranscriptResource = {
      kind: 'agent-transcript',
      key: 'agent:agent-01',
      title: 'Agent 01',
      status: 'running',
      attention: false,
      locator: {
        projectId: 'project-1',
        taskId: 'task-1',
        runId: 'run-1',
        roundId: 'round-1',
        nodeId: 'node-1',
        attemptId: 'attempt-1',
        branchId: 'agent-01',
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<AgentConversationPanel resource={resource} />);
      });
      const renderer = container.querySelector('output');
      expect(renderer?.getAttribute('data-branch-id')).toBe('agent-01');
      expect(renderer?.getAttribute('data-read-only')).toBe('true');
      expect(renderer?.getAttribute('data-show-system-prompt')).toBe('false');
      expect(renderer?.getAttribute('data-show-raw-frames')).toBe('false');
      expect(renderer?.getAttribute('data-usage-compact')).toBe('true');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
