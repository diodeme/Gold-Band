import { describe, expect, it } from 'vitest';
import { ConversationRunModePersistence } from '@/lib/conversation-run-mode-persistence';
import type { ConversationRunModeVm } from '@/types';

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe('ConversationRunModePersistence', () => {
  it('serializes rapid Direct updates inside the same workspace', async () => {
    const firstSave = deferred();
    const saved: Array<{ projectId: string; mode: ConversationRunModeVm }> = [];
    let callCount = 0;
    const persistence = new ConversationRunModePersistence(async (projectId, mode) => {
      callCount += 1;
      if (callCount === 1) await firstSave.promise;
      saved.push({ projectId, mode });
    });

    const model = persistence.enqueue('workspace-a', {
      mode: 'direct',
      directConfig: { agentType: 'claude-acp', modelId: 'sonnet' },
    });
    const permission = persistence.enqueue('workspace-a', {
      mode: 'direct',
      directConfig: {
        agentType: 'claude-acp',
        modelId: 'sonnet',
        permissionMode: 'bypassPermissions',
      },
    });

    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(callCount).toBe(1);
    firstSave.resolve();
    await Promise.all([model, permission]);
    expect(saved.map(({ mode }) => mode.directConfig)).toEqual([
      { agentType: 'claude-acp', modelId: 'sonnet' },
      { agentType: 'claude-acp', modelId: 'sonnet', permissionMode: 'bypassPermissions' },
    ]);
  });

  it('waits for the latest workspace save before reloading that workspace', async () => {
    const save = deferred();
    const persistence = new ConversationRunModePersistence(() => save.promise);
    const pending = persistence.enqueue('workspace-a', { mode: 'direct' });
    let reloadReady = false;
    const wait = persistence.waitFor('workspace-a').then(() => {
      reloadReady = true;
    });

    await Promise.resolve();
    expect(reloadReady).toBe(false);
    save.resolve();
    await Promise.all([pending, wait]);
    expect(reloadReady).toBe(true);
  });

  it('does not block a different workspace behind the current workspace queue', async () => {
    const workspaceASave = deferred();
    const saved: string[] = [];
    const persistence = new ConversationRunModePersistence(async (projectId) => {
      if (projectId === 'workspace-a') await workspaceASave.promise;
      saved.push(projectId);
    });

    const a = persistence.enqueue('workspace-a', { mode: 'direct' });
    const b = persistence.enqueue('workspace-b', { mode: 'auto' });
    await b;
    expect(saved).toEqual(['workspace-b']);
    workspaceASave.resolve();
    await a;
    expect(saved).toEqual(['workspace-b', 'workspace-a']);
  });
});
