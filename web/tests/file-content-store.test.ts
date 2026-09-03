import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  readFileResource: vi.fn(),
  resolveMarkdownImage: vi.fn(),
  writeFileResource: vi.fn(),
  releaseExternalFileAccess: vi.fn().mockResolvedValue(undefined),
  releaseWorkspaceFilePreview: vi.fn().mockResolvedValue(undefined),
  renewExternalFileAccess: vi.fn(),
  startWorkspaceFileWatch: vi.fn().mockResolvedValue(undefined),
  stopWorkspaceFileWatch: vi.fn().mockResolvedValue(undefined),
  subscribeWorkspaceFileChanges: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@/api', () => api);

import { FileContentStore } from '@/components/workspace/files/file-content-store';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import type { FileWorkspaceResource } from '@/components/workspace/right-workspace-context';
import type { ExternalFileAccessGrantVm, FileRevisionVm, TextFileSnapshotVm, WorkspaceFileChangedEventVm } from '@/types';

const revision = (hash: string): FileRevisionVm => ({ byteLength: 5, modifiedAtNs: hash, contentHash: hash });
const resource: FileWorkspaceResource = {
  kind: 'file',
  key: 'file:project-1:D:/repo/file.txt',
  scopeKey: 'draft:project-1',
  projectId: 'project-1',
  title: 'file.txt',
  attention: false,
  locator: { projectId: 'project-1', canonicalPath: 'D:/repo/file.txt', relativePath: 'file.txt', scope: 'workspace' },
  target: null,
  targetRevision: 0,
};

function snapshot(content = 'start', disk = revision('disk-1')): TextFileSnapshotVm {
  return {
    kind: 'text',
    locator: resource.locator,
    name: 'file.txt',
    revision: disk,
    content,
    encoding: 'utf-8',
    language: 'text',
    lineEnding: 'lf',
    editable: true,
    limitationCode: null,
    externalAccessGrant: null,
  };
}

const externalGrant = (token: string): ExternalFileAccessGrantVm => ({
  token,
  permissions: ['read', 'write'],
  expiresAtMs: String(Date.now() + 30 * 60 * 1_000),
});

const externalResource: FileWorkspaceResource = {
  ...resource,
  key: 'file:project-1:d:/outside/file.txt',
  locator: {
    projectId: 'project-1',
    canonicalPath: 'D:/outside/file.txt',
    relativePath: null,
    scope: 'external',
  },
};

function externalSnapshot(content = 'outside', grant = externalGrant('grant-1')): TextFileSnapshotVm {
  return {
    ...snapshot(content),
    locator: externalResource.locator,
    externalAccessGrant: grant,
  };
}

function createStore() {
  const store = new FileContentStore();
  store.configure({ ...FALLBACK_WORKSPACE_FILES, autoSaveDelayMs: 300 });
  return store;
}

const previewGrant = (token: string, expiresInMs = 5 * 60 * 1_000) => ({
  token,
  expiresAtMs: String(Date.now() + expiresInMs),
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  api.readFileResource.mockResolvedValue(snapshot());
  api.resolveMarkdownImage.mockResolvedValue({
    kind: 'ready',
    canonicalPath: 'D:/repo/image.png',
    previewGrant: previewGrant('preview-1'),
    mimeType: 'image/png',
    width: 640,
    height: 360,
    animated: false,
  });
  api.writeFileResource.mockResolvedValue(revision('disk-next'));
  api.releaseExternalFileAccess.mockResolvedValue(undefined);
  api.releaseWorkspaceFilePreview.mockResolvedValue(undefined);
  api.startWorkspaceFileWatch.mockResolvedValue(undefined);
  api.stopWorkspaceFileWatch.mockResolvedValue(undefined);
  api.subscribeWorkspaceFileChanges.mockResolvedValue(() => {});
  api.renewExternalFileAccess.mockResolvedValue(externalGrant('grant-renewed'));
});

afterEach(() => vi.useRealTimers());

describe('FileContentStore autosave contract', () => {
  it('coalesces rapid edits for 300ms and writes only the latest content', async () => {
    const store = createStore();
    await store.load(resource);
    store.updateText(resource.key, 'first');
    store.updateText(resource.key, 'latest');

    await vi.advanceTimersByTimeAsync(299);
    expect(api.writeFileResource).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);

    expect(api.writeFileResource).toHaveBeenCalledTimes(1);
    expect(api.writeFileResource.mock.calls[0]?.[0].content).toBe('latest');
    expect(store.snapshot(resource.key).saveState.kind).toBe('clean');
  });

  it('serializes a newer edit behind an in-flight write without losing it', async () => {
    const store = createStore();
    await store.load(resource);
    let finishFirst!: (revision: FileRevisionVm) => void;
    const firstWrite = new Promise<FileRevisionVm>((resolve) => { finishFirst = resolve; });
    api.writeFileResource
      .mockImplementationOnce(() => firstWrite)
      .mockResolvedValueOnce(revision('disk-3'));

    store.updateText(resource.key, 'first');
    const saving = store.flush(resource.key);
    await Promise.resolve();
    expect(api.writeFileResource).toHaveBeenCalledTimes(1);

    store.updateText(resource.key, 'second');
    finishFirst(revision('disk-2'));
    await saving;

    expect(api.writeFileResource).toHaveBeenCalledTimes(2);
    expect(api.writeFileResource.mock.calls[1]?.[0].content).toBe('second');
    expect(store.snapshot(resource.key).snapshot?.kind === 'text' && store.snapshot(resource.key).snapshot.content).toBe('second');
  });

  it('keeps a conflicted tab open when close cannot flush safely', async () => {
    const store = createStore();
    await store.load(resource);
    api.writeFileResource.mockRejectedValueOnce({ code: 'workspace-file.changed-on-disk' });
    store.updateText(resource.key, 'local edit');

    await expect(store.close(resource.key)).resolves.toBe(false);
    expect(store.snapshot(resource.key).saveState.kind).toBe('conflict');
    expect(store.snapshot(resource.key).status).toBe('ready');

    api.writeFileResource.mockClear();
    store.updateText(resource.key, 'newer local edit');
    await vi.advanceTimersByTimeAsync(1_000);
    expect(store.snapshot(resource.key).saveState.kind).toBe('conflict');
    expect(api.writeFileResource).not.toHaveBeenCalled();
  });

  it('persists undo and redo results through the same content update contract', async () => {
    const store = createStore();
    await store.load(resource);
    store.updateText(resource.key, 'typed');
    store.updateText(resource.key, 'start');
    await store.flush(resource.key);
    store.updateText(resource.key, 'typed');
    await store.flush(resource.key);

    expect(api.writeFileResource.mock.calls.map((call) => call[0].content)).toEqual(['start', 'typed']);
  });

  it('creates a new undo boundary when disk content is reloaded', async () => {
    const store = createStore();
    await store.load(resource);
    const previousContentRevision = store.snapshot(resource.key).contentRevision;
    store.persistEditorState(resource.key, { history: ['old'] }, previousContentRevision);
    api.readFileResource.mockResolvedValueOnce(snapshot('disk replacement', revision('disk-2')));

    await store.load(resource, false, true);
    store.persistEditorState(resource.key, { history: ['stale cleanup'] }, previousContentRevision);

    expect(store.editorState(resource.key)).toBeNull();
    expect(store.snapshot(resource.key).snapshot?.kind === 'text'
      && store.snapshot(resource.key).snapshot.content).toBe('disk replacement');
  });

  it('flushes a deactivated file without releasing its content, history, or grant', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce(externalSnapshot());
    await store.load(externalResource);
    store.persistEditorState(externalResource.key, { history: ['typed'] });
    store.updateText(externalResource.key, 'latest outside');

    await expect(store.flush(externalResource.key)).resolves.toBe(true);
    expect(store.snapshot(externalResource.key).status).toBe('ready');
    expect(store.editorState(externalResource.key)).toEqual({ history: ['typed'] });
    expect(api.releaseExternalFileAccess).not.toHaveBeenCalled();

    await expect(store.close(externalResource.key)).resolves.toBe(true);
    expect(api.releaseExternalFileAccess).toHaveBeenCalledWith('grant-1');
    expect(store.snapshot(externalResource.key).status).toBe('idle');
  });

  it('reauthorizes without reloading or discarding the latest in-memory edit', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce(externalSnapshot());
    await store.load(externalResource);
    store.updateText(externalResource.key, 'unsaved latest');
    api.writeFileResource.mockClear();

    await expect(store.reauthorize(externalResource.key, externalGrant('grant-2'))).resolves.toBe(true);

    expect(api.readFileResource).toHaveBeenCalledTimes(1);
    expect(api.writeFileResource).toHaveBeenCalledWith(expect.objectContaining({
      content: 'unsaved latest',
      externalAccessToken: 'grant-2',
    }));
    expect(store.snapshot(externalResource.key).snapshot?.kind === 'text'
      && store.snapshot(externalResource.key).snapshot.content).toBe('unsaved latest');
  });

  it('releases a primed external grant even when the Tab closes before loading', async () => {
    const store = createStore();
    store.primeExternalGrant(
      externalResource.key,
      externalResource.projectId,
      externalResource.locator.canonicalPath,
      externalGrant('primed-only'),
    );

    await expect(store.close(externalResource.key)).resolves.toBe(true);
    expect(api.releaseExternalFileAccess).toHaveBeenCalledWith('primed-only');
  });

  it('renews an external grant before expiry and releases the rotated token', async () => {
    const store = createStore();
    const expiring = {
      ...externalGrant('grant-expiring'),
      expiresAtMs: String(Date.now() + 61_000),
    };
    api.readFileResource.mockResolvedValueOnce(externalSnapshot('outside', expiring));
    api.renewExternalFileAccess.mockResolvedValueOnce(externalGrant('grant-rotated'));
    await store.load(externalResource);

    await vi.advanceTimersByTimeAsync(1_000);
    expect(api.renewExternalFileAccess).toHaveBeenCalledWith('grant-expiring');
    expect(store.snapshot(externalResource.key).snapshot?.externalAccessGrant?.token).toBe('grant-rotated');

    await store.close(externalResource.key);
    expect(api.releaseExternalFileAccess).toHaveBeenCalledWith('grant-rotated');
  });

  it('reloads a clean file change and turns a pending edit into a conflict', async () => {
    const store = createStore();
    let onChange: ((event: WorkspaceFileChangedEventVm) => void) | null = null;
    api.subscribeWorkspaceFileChanges.mockImplementationOnce(async (listener) => {
      onChange = listener;
      return () => {};
    });
    await store.load(resource);
    await store.startProjectWatch(resource.projectId);
    api.readFileResource.mockResolvedValueOnce(snapshot('external clean', revision('disk-2')));
    onChange?.({
      projectId: resource.projectId,
      canonicalPath: resource.locator.canonicalPath,
      kind: 'modified',
      revision: revision('disk-2'),
      operationId: null,
    });
    await vi.waitFor(() => expect(store.snapshot(resource.key).snapshot?.kind === 'text'
      && store.snapshot(resource.key).snapshot.content).toBe('external clean'));

    store.updateText(resource.key, 'pending local');
    onChange?.({
      projectId: resource.projectId,
      canonicalPath: resource.locator.canonicalPath,
      kind: 'modified',
      revision: revision('disk-3'),
      operationId: null,
    });
    expect(store.snapshot(resource.key).saveState.kind).toBe('conflict');

    onChange?.({
      projectId: resource.projectId,
      canonicalPath: resource.locator.canonicalPath,
      kind: 'removed',
      revision: null,
      operationId: null,
    });
    expect(store.snapshot(resource.key).status).toBe('ready');
    expect(store.snapshot(resource.key).snapshot?.kind === 'text'
      && store.snapshot(resource.key).snapshot.content).toBe('pending local');
    expect(store.snapshot(resource.key).saveState).toMatchObject({
      kind: 'error',
      errorCode: 'workspace-file.not-found',
    });
  });

  it('reconciles clean cached content after the workspace watcher becomes active', async () => {
    const store = createStore();
    await store.load(resource);
    api.readFileResource.mockResolvedValueOnce(snapshot('changed while inactive', revision('disk-2')));

    const reconciliation = store.reconcile(resource.key);

    expect(store.snapshot(resource.key).status).toBe('ready');
    await reconciliation;
    expect(store.snapshot(resource.key).snapshot?.kind === 'text'
      && store.snapshot(resource.key).snapshot.content).toBe('changed while inactive');
    expect(api.readFileResource).toHaveBeenCalledTimes(2);
  });

  it('rolls back a failed watcher activation so the next activation can retry', async () => {
    const store = createStore();
    api.startWorkspaceFileWatch
      .mockRejectedValueOnce({ code: 'workspace-file.watch-failed' })
      .mockResolvedValueOnce(undefined);

    await expect(store.startProjectWatch(resource.projectId)).rejects.toMatchObject({
      code: 'workspace-file.watch-failed',
    });
    await store.startProjectWatch(resource.projectId);
    await store.stopProjectWatch(resource.projectId);

    expect(api.startWorkspaceFileWatch).toHaveBeenCalledTimes(2);
    expect(api.stopWorkspaceFileWatch).toHaveBeenCalledTimes(1);
  });

  it('reconciles clean cached content after a watcher scope invalidation', async () => {
    const store = createStore();
    let onChange: ((event: WorkspaceFileChangedEventVm) => void) | null = null;
    api.subscribeWorkspaceFileChanges.mockImplementationOnce(async (listener) => {
      onChange = listener;
      return () => {};
    });
    await store.load(resource);
    await store.startProjectWatch(resource.projectId);
    api.readFileResource.mockResolvedValueOnce(snapshot('recovered after overflow', revision('disk-2')));

    onChange?.({
      projectId: resource.projectId,
      canonicalPath: 'D:/repo',
      kind: 'invalidated',
      revision: null,
      operationId: null,
    });

    await vi.waitFor(() => expect(store.snapshot(resource.key).snapshot?.kind === 'text'
      && store.snapshot(resource.key).snapshot.content).toBe('recovered after overflow'));
  });
});

describe('FileContentStore Markdown runtime contract', () => {
  const markdownResource: FileWorkspaceResource = {
    ...resource,
    key: 'file:project-1:D:/repo/readme.md',
    title: 'readme.md',
    locator: { ...resource.locator, canonicalPath: 'D:/repo/readme.md', relativePath: 'readme.md' },
  };

  it('keeps Markdown mode in the file runtime and rejects live preview above the configured threshold', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('# Title'), locator: markdownResource.locator, language: 'markdown' });
    await store.load(markdownResource);

    expect(store.markdownMode(markdownResource.key)).toBe('live-preview');
    store.setMarkdownMode(markdownResource.key, 'source');
    expect(store.markdownMode(markdownResource.key)).toBe('source');

    store.configure({ ...FALLBACK_WORKSPACE_FILES, markdownLivePreviewMaxChars: 3 });
    store.setMarkdownMode(markdownResource.key, 'live-preview');
    expect(store.markdownMode(markdownResource.key)).toBe('source');
  });

  it('resolves embedded images through preview tokens and releases them when the reference disappears', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![diagram](image.png)'), locator: markdownResource.locator, language: 'markdown' });
    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['image.png']);

    expect(api.resolveMarkdownImage).toHaveBeenCalledWith(expect.objectContaining({
      markdownCanonicalPath: 'D:/repo/readme.md',
      rawSrc: 'image.png',
    }));
    expect(store.markdownImages(markdownResource.key).get('image.png')?.kind).toBe('ready');

    await store.syncMarkdownImages(markdownResource.key, []);
    expect(api.releaseWorkspaceFilePreview).toHaveBeenCalledWith('preview-1');
    expect(store.markdownImages(markdownResource.key).size).toBe(0);
  });

  it('confirms all currently referenced directory-external images once and retries exact targets', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![outside](../outside.png)'), locator: markdownResource.locator, language: 'markdown' });
    api.resolveMarkdownImage
      .mockResolvedValueOnce({ kind: 'approvalRequired', canonicalPath: 'D:/outside.png', reason: 'outside-document-directory' })
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/outside.png', previewGrant: previewGrant('preview-outside'),
        mimeType: 'image/png', width: 100, height: 80, animated: false,
      });
    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['../outside.png']);
    expect(store.markdownImages(markdownResource.key).get('../outside.png')?.kind).toBe('approvalRequired');

    await store.approveMarkdownImages(markdownResource.key);
    expect(api.resolveMarkdownImage.mock.calls[1]?.[0].approvedExternalTargets).toEqual(['D:/outside.png']);
    expect(store.markdownImages(markdownResource.key).get('../outside.png')?.kind).toBe('ready');
  });

  it('renews Markdown preview grants before expiry and releases the old token after replacement', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![diagram](image.png)'), locator: markdownResource.locator, language: 'markdown' });
    api.resolveMarkdownImage
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-old'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      })
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-new'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      });

    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['image.png']);
    await vi.advanceTimersByTimeAsync(4 * 60 * 1_000);

    const image = store.markdownImages(markdownResource.key).get('image.png');
    expect(image?.kind === 'ready' && image.previewGrant.token).toBe('preview-new');
    expect(api.releaseWorkspaceFilePreview).toHaveBeenCalledWith('preview-old');
  });

  it('keeps the current Markdown preview grant when renewal fails and retries later', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![diagram](image.png)'), locator: markdownResource.locator, language: 'markdown' });
    api.resolveMarkdownImage
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-current'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      })
      .mockRejectedValueOnce({ code: 'workspace-file.runtime-unavailable' })
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-recovered'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      });

    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['image.png']);
    await vi.advanceTimersByTimeAsync(4 * 60 * 1_000);
    let image = store.markdownImages(markdownResource.key).get('image.png');
    expect(image?.kind === 'ready' && image.previewGrant.token).toBe('preview-current');

    await vi.advanceTimersByTimeAsync(15_000);
    image = store.markdownImages(markdownResource.key).get('image.png');
    expect(image?.kind === 'ready' && image.previewGrant.token).toBe('preview-recovered');
  });

  it('does not send remote Markdown image URLs to the local preview resolver', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![remote](https://example.com/a.png)'), locator: markdownResource.locator, language: 'markdown' });
    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['https://example.com/a.png']);

    expect(api.resolveMarkdownImage).not.toHaveBeenCalled();
    expect(store.markdownImages(markdownResource.key).size).toBe(0);
  });

  it('reissues the active grant after an image load error but ignores stale widget errors', async () => {
    const store = createStore();
    api.readFileResource.mockResolvedValueOnce({ ...snapshot('![diagram](image.png)'), locator: markdownResource.locator, language: 'markdown' });
    api.resolveMarkdownImage
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-broken'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      })
      .mockResolvedValueOnce({
        kind: 'ready', canonicalPath: 'D:/repo/image.png', previewGrant: previewGrant('preview-reissued'),
        mimeType: 'image/png', width: 640, height: 360, animated: false,
      });

    await store.load(markdownResource);
    await store.syncMarkdownImages(markdownResource.key, ['image.png']);
    await store.refreshMarkdownImages(markdownResource.key, 'already-replaced');
    expect(api.resolveMarkdownImage).toHaveBeenCalledTimes(1);

    await store.refreshMarkdownImages(markdownResource.key, 'preview-broken');
    const image = store.markdownImages(markdownResource.key).get('image.png');
    expect(image?.kind === 'ready' && image.previewGrant.token).toBe('preview-reissued');
  });
});
