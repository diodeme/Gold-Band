import { afterEach, describe, expect, it, vi } from 'vitest';
import { MarkdownImageLeases, type LocalImageState } from '@/lib/markdown-image-leases';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import type { MarkdownImagePreviewVm, ResolveMarkdownImageInput } from '@/types';

const root = { projectId: 'project-a', workspacePath: 'D:/repo/.wt/one' };
const ready = (token: string, width = 3, height = 2): MarkdownImagePreviewVm => ({
  kind: 'ready', canonicalPath: 'D:/repo/.wt/one/docs/image.png',
  previewGrant: { token, expiresAtMs: String(Date.now() + 300_000) },
  width, height, mimeType: 'image/png', animated: false,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function fixture(limits = FALLBACK_WORKSPACE_FILES) {
  const io = {
    resolve: vi.fn<(input: ResolveMarkdownImageInput) => Promise<MarkdownImagePreviewVm>>(),
    release: vi.fn(async (_token: string) => undefined),
  };
  const owner = new MarkdownImageLeases(root, limits, io);
  owners.push(owner);
  return { owner, io };
}
const owners: MarkdownImageLeases[] = [];
afterEach(() => { owners.splice(0).forEach(owner => owner.clear()); vi.useRealTimers(); });
const settle = async () => { await Promise.resolve(); await Promise.resolve(); };

describe('conversation local image leases', () => {
  it('single-flights shared sources and releases only after the final visible reference', async () => {
    const { owner, io } = fixture();
    io.resolve.mockResolvedValue(ready('token'));
    const one = vi.fn();
    const a = owner.acquire('docs/image.png', one);
    const b = owner.acquire('docs/image.png', vi.fn());
    await settle();
    expect(io.resolve).toHaveBeenCalledExactlyOnceWith({ ...root, source: { kind: 'workspace' }, rawSrc: 'docs/image.png' });
    expect(one).toHaveBeenLastCalledWith(expect.objectContaining({ kind: 'ready' }));
    a.release(); a.release();
    expect(io.release).not.toHaveBeenCalled();
    b.release();
    expect(io.release).toHaveBeenCalledExactlyOnceWith('token');
  });

  it('reclaims a late token after a scope is cleared without publishing stale state', async () => {
    const { owner, io } = fixture();
    const pending = deferred<MarkdownImagePreviewVm>();
    io.resolve.mockReturnValue(pending.promise);
    const listener = vi.fn();
    owner.acquire('docs/image.png', listener);
    owner.clear();
    pending.resolve(ready('late'));
    await settle();
    expect(listener).toHaveBeenCalledTimes(1);
    expect(io.release).toHaveBeenCalledExactlyOnceWith('late');
  });

  it('bounds queued references and simultaneous reads, dropping released queue entries', async () => {
    const { owner, io } = fixture({ ...FALLBACK_WORKSPACE_FILES, markdownEmbeddedImageLimit: 3, markdownEmbeddedImageMaxConcurrent: 1 });
    const pending = deferred<MarkdownImagePreviewVm>();
    io.resolve.mockReturnValueOnce(pending.promise).mockResolvedValue(ready('second'));
    owner.acquire('a.png', vi.fn());
    const dropped = owner.acquire('b.png', vi.fn());
    owner.acquire('c.png', vi.fn());
    const overflow = vi.fn();
    owner.acquire('d.png', overflow);
    expect(overflow).toHaveBeenCalledWith({ kind: 'error', code: 'workspace-file.too-large' });
    expect(io.resolve).toHaveBeenCalledTimes(1);
    dropped.release();
    pending.resolve(ready('first'));
    await settle();
    expect(io.resolve).toHaveBeenCalledTimes(2);
    expect(io.resolve.mock.calls[1][0].rawSrc).toBe('c.png');
  });

  it('renews before expiry and releases the old grant after publishing its replacement', async () => {
    vi.useFakeTimers();
    const { owner, io } = fixture();
    io.resolve.mockResolvedValueOnce(ready('old')).mockImplementation(async () => ready('new'));
    const states: LocalImageState[] = [];
    owner.acquire('a.png', state => { states.push(state); });
    await settle();
    await vi.advanceTimersByTimeAsync(240_000);
    expect(states.at(-1)).toMatchObject({ kind: 'ready', previewGrant: { token: 'new' } });
    expect(io.release).toHaveBeenCalledWith('old');
    owner.clear();
    await vi.advanceTimersByTimeAsync(600_000);
    expect(io.resolve).toHaveBeenCalledTimes(2);
    expect(io.release).toHaveBeenCalledWith('new');
  });

  it('admits an aggregate pixel budget before images reach the DOM', async () => {
    const { owner, io } = fixture({ ...FALLBACK_WORKSPACE_FILES, imagePreviewMaxPixels: 10 });
    io.resolve.mockResolvedValueOnce(ready('one')).mockResolvedValueOnce(ready('two'));
    owner.acquire('a.png', vi.fn());
    const second = vi.fn();
    owner.acquire('b.png', second);
    await settle();
    expect(second).toHaveBeenLastCalledWith({ kind: 'error', code: 'workspace-file.too-large' });
    expect(io.release).toHaveBeenCalledWith('two');
  });

  it('isolates same-name files across two project/worktree owners', async () => {
    const { owner, io } = fixture();
    io.resolve.mockResolvedValue(ready('one'));
    const other = new MarkdownImageLeases({ projectId: 'project-b', workspacePath: null }, FALLBACK_WORKSPACE_FILES, io);
    owners.push(other);
    owner.acquire('a.png', vi.fn());
    other.acquire('a.png', vi.fn());
    await settle();
    expect(io.resolve.mock.calls.map(([input]) => input.projectId)).toEqual(['project-a', 'project-b']);
    expect(io.resolve.mock.calls.map(([input]) => input.workspacePath)).toEqual(['D:/repo/.wt/one', null]);
  });
});
