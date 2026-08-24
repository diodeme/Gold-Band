/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ConversationAssetWorkspaceResource } from '@/components/workspace/right-workspace-context';
import '@/i18n';

const api = vi.hoisted(() => ({
  showArtifact: vi.fn(),
  showConversationAttachment: vi.fn(),
  showConversationMessageAttachment: vi.fn(),
}));

vi.mock('@/api', () => api);

vi.mock('@/components/workspace/files/ReadonlyTextWorkspaceViewer', () => ({
  ReadonlyTextWorkspaceViewer: (props: { documentKey: string; name: string; value: string }) => (
    <div data-testid="readonly-text-workspace" data-name={props.name}>{props.value}</div>
  ),
}));

vi.mock('@/components/workspace/files/WorkspaceImageCanvas', () => ({
  WorkspaceImageCanvas: () => <div data-testid="workspace-image" />,
}));

import { ConversationAssetWorkspacePanel } from '@/components/workspace/files/ConversationAssetWorkspacePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const resource: ConversationAssetWorkspaceResource = {
  kind: 'conversation-asset',
  key: 'conversation-asset:message:notes',
  scopeKey: 'conversation:project-1:task-1:run-1',
  title: 'notes.md',
  attention: false,
  locator: {
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    branchId: 'branch-1',
  },
  assetKind: 'message-attachment',
  name: 'notes.md',
  path: 'user-inputs/notes.md',
};

describe('conversation asset workspace panel', () => {
  afterEach(() => {
    vi.clearAllMocks();
    document.body.replaceChildren();
  });

  it('loads a message text attachment in the right workspace shared viewer', async () => {
    api.showConversationMessageAttachment.mockResolvedValue({
      title: 'notes.md',
      kind: 'message-attachment',
      content: '# Sent attachment',
      metadata: {},
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ConversationAssetWorkspacePanel resource={resource} />);
      });

      expect(api.showConversationMessageAttachment).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        'notes.md',
        'user-inputs/notes.md',
        undefined,
        undefined,
      );
      const viewer = container.querySelector<HTMLElement>('[data-testid="readonly-text-workspace"]');
      expect(viewer?.dataset.name).toBe('notes.md');
      expect(viewer?.textContent).toBe('# Sent attachment');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
