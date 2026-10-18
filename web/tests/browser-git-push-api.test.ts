import { afterEach, describe, expect, it, vi } from 'vitest';
import { browserApi } from '@/api/browser';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('browser Git push fixture contract', () => {
  it('establishes upstream after first publication and projects zero ahead', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('window', { location: { search: '?gitPush=no-upstream' } });
    const projectId = 'push-first-publish';
    const pending = browserApi.prepareGitPush(projectId, '/preview/first-publish', { kind: 'upstream-sync', branch: 'feature/source-control', remote: 'origin' });
    await vi.runAllTimersAsync();
    const prepared = await pending;
    expect(prepared.target.setUpstream).toBe(true);
    const queued = await browserApi.startGitOperation(projectId, '/preview/first-publish', { kind: 'push', ...prepared });
    await vi.runAllTimersAsync();
    expect((await browserApi.getGitOperation(queued.operationId)).status).toBe('succeeded');
    const snapshot = await browserApi.getSourceControlSnapshot(projectId, '/preview/first-publish');
    expect(snapshot.repository.upstream).toEqual({ name: 'origin/feature/source-control', ahead: 0, behind: 0 });
    expect(snapshot.refs.find((ref) => ref.shortName === 'feature/source-control')?.upstream).toBe('origin/feature/source-control');
  });
});
