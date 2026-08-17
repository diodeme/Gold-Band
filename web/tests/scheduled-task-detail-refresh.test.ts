import { describe, expect, it, vi } from 'vitest';
import { createScheduledTaskDetailRefreshCoordinator } from '@/lib/scheduled-task-detail-refresh';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}

describe('scheduled task detail refresh coordinator', () => {
  it('keeps one request in flight and coalesces a burst into one latest follow-up', async () => {
    const stale = deferred<{ history: string; diagnostics: number }>();
    const fresh = deferred<{ history: string; diagnostics: number }>();
    const load = vi.fn()
      .mockReturnValueOnce(stale.promise)
      .mockReturnValueOnce(fresh.promise);
    const committed: Array<{ history: string; diagnostics: number }> = [];
    const fail = vi.fn();
    const coordinator = createScheduledTaskDetailRefreshCoordinator({
      load,
      commit: (result) => committed.push(result),
      fail,
    });

    coordinator.request({ event: 1 });
    coordinator.request({ event: 2 });
    coordinator.request({ event: 3 });

    expect(load).toHaveBeenCalledTimes(1);

    stale.reject(new Error('obsolete refresh failed'));
    await flushPromises();

    expect(fail).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(2);
    expect(load).toHaveBeenLastCalledWith({ event: 3 });

    fresh.resolve({ history: 'fresh-history', diagnostics: 22 });
    await flushPromises();

    expect(committed).toEqual([{ history: 'fresh-history', diagnostics: 22 }]);
  });

  it('does not let an old history and diagnostics result overwrite a foreground page', async () => {
    const stale = deferred<{ history: string; diagnostics: number }>();
    const committed: Array<{ history: string; diagnostics: number }> = [];
    const fail = vi.fn();
    const coordinator = createScheduledTaskDetailRefreshCoordinator({
      load: () => stale.promise,
      commit: (result) => committed.push(result),
      fail,
    });

    coordinator.request({ source: 'occurrence-event' });
    const foregroundGeneration = coordinator.beginForegroundRequest();
    if (coordinator.isCurrent(foregroundGeneration)) {
      committed.push({ history: 'page-2', diagnostics: 22 });
    }

    stale.resolve({ history: 'late-event-history', diagnostics: 91 });
    await flushPromises();

    expect(committed).toEqual([{ history: 'page-2', diagnostics: 22 }]);
    expect(fail).not.toHaveBeenCalled();
  });

  it('suppresses an old failure after a task switch and accepts only the new generation', async () => {
    const stale = deferred<string>();
    const fail = vi.fn();
    const committed: string[] = [];
    const coordinator = createScheduledTaskDetailRefreshCoordinator({
      load: () => stale.promise,
      commit: (result) => committed.push(result),
      fail,
    });

    coordinator.request({ taskId: 'task-1' });
    const oldTaskGeneration = coordinator.beginForegroundRequest();
    const newTaskGeneration = coordinator.beginForegroundRequest();
    if (coordinator.isCurrent(newTaskGeneration)) committed.push('task-2');

    expect(coordinator.isCurrent(oldTaskGeneration)).toBe(false);

    stale.reject(new Error('task-1 failed late'));
    await flushPromises();

    expect(fail).not.toHaveBeenCalled();
    expect(committed).toEqual(['task-2']);
  });
});
