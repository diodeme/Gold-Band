import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { resolveConversationWorkspaceRemovalTransition } from '@/lib/conversation-workspace-removal';

describe('conversation workspace removal', () => {
  it('navigates away from a run in the removed workspace and selects the backend fallback', () => {
    expect(resolveConversationWorkspaceRemovalTransition({
      removedProjectId: 'workspace-a',
      lastActiveWorkspaceId: 'workspace-b',
      activeWorkspaceId: 'workspace-a',
      draftWorkspaceId: 'workspace-a',
      page: { kind: 'conversation-run', projectId: 'workspace-a', taskId: 'task-1', runId: 'run-1' },
    })).toEqual({
      activeWorkspaceId: 'workspace-b',
      draftWorkspaceId: 'workspace-b',
      navigateHome: true,
    });
  });

  it('preserves an unrelated active workspace and page', () => {
    expect(resolveConversationWorkspaceRemovalTransition({
      removedProjectId: 'workspace-a',
      lastActiveWorkspaceId: 'workspace-b',
      activeWorkspaceId: 'workspace-b',
      draftWorkspaceId: null,
      page: { kind: 'settings' },
    })).toEqual({
      activeWorkspaceId: 'workspace-b',
      draftWorkspaceId: null,
      navigateHome: false,
    });
  });

  it('supports removing the final workspace', () => {
    expect(resolveConversationWorkspaceRemovalTransition({
      removedProjectId: 'workspace-a',
      lastActiveWorkspaceId: null,
      activeWorkspaceId: 'workspace-a',
      draftWorkspaceId: 'workspace-a',
      page: { kind: 'conversation-home' },
    })).toEqual({
      activeWorkspaceId: null,
      draftWorkspaceId: null,
      navigateHome: false,
    });
  });

  it('requires confirmation and awaits one pending removal before closing', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'),
      'utf8',
    );

    expect(source).toContain('setWorkspaceToRemove(ws)');
    expect(source).not.toContain('onRemoveWorkspace(ws.projectId)');
    expect(source).toContain('event.preventDefault()');
    expect(source).toContain('workspaceRemovalPending) return');
    expect(source).toContain('await onRemoveWorkspace(workspaceToRemove.projectId)');
    expect(source).toContain('conversation.sidebar.removeWorkspaceDescription');
  });
});
