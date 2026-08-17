import { describe, expect, it, vi } from 'vitest';
import { GitHubOperationEventStore } from '@/components/workspace/source-control/github-operation-store';
import type { GitHubOperationVm } from '@/types';

describe('GitHub operation event store', () => {
  it('shares one upstream subscription and reconciles an event that arrives before command return', async () => {
    let emit: ((operation: GitHubOperationVm) => void) | null = null;
    const subscribeUpdates = vi.fn(async (listener: (operation: GitHubOperationVm) => void) => {
      emit = listener;
      return () => { emit = null; };
    });
    const store = new GitHubOperationEventStore(subscribeUpdates);
    const firstListener = vi.fn();
    const secondListener = vi.fn();
    store.subscribe(firstListener);
    store.subscribe(secondListener);
    await vi.waitFor(() => expect(subscribeUpdates).toHaveBeenCalledTimes(1));

    const queued = operation('queued');
    const succeeded = { ...queued, status: 'succeeded' as const, cancelable: false };
    emit?.(succeeded);

    expect(store.reconcile(queued)).toEqual(succeeded);
    expect(firstListener).toHaveBeenCalledWith(succeeded);
    expect(secondListener).toHaveBeenCalledWith(succeeded);
  });
});

function operation(status: GitHubOperationVm['status']): GitHubOperationVm {
  return {
    operationId: 'github-operation-1',
    kind: 'pr-create',
    host: 'github.com',
    status,
    cancelable: status === 'queued' || status === 'running',
    startedAt: null,
    completedAt: null,
    error: null,
    resultUrl: null,
  };
}
