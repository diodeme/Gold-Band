import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  listWorkspaceDirectory: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
}));

vi.mock('@/api', () => api);

import { FileExplorerStore, fileTreeView } from '@/components/workspace/files/file-explorer-store';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileChangedEventVm } from '@/types';

const directory = (name: string, relativePath = name): WorkspaceDirectoryEntryVm => ({
  name,
  relativePath,
  canonicalPath: `D:\\repo\\${relativePath.replaceAll('/', '\\')}`,
  kind: 'directory',
  hasChildren: true,
  byteLength: null,
  modifiedAtNs: '1',
});

const file = (name: string, relativePath = name): WorkspaceDirectoryEntryVm => ({
  name,
  relativePath,
  canonicalPath: `D:\\repo\\${relativePath.replaceAll('/', '\\')}`,
  kind: 'file',
  hasChildren: false,
  byteLength: 10,
  modifiedAtNs: '1',
});

function createStore() {
  const store = new FileExplorerStore();
  store.configure({ ...FALLBACK_WORKSPACE_FILES, searchDebounceMs: 200 });
  return store;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  api.listWorkspaceDirectory.mockImplementation(async (_projectId: string, path: string) => (
    path === '' ? [directory('src'), file('README.md')] : path === 'src' ? [directory('nested', 'src/nested')] : [file('main.rs', 'src/nested/main.rs')]
  ));
});

afterEach(() => vi.useRealTimers());

describe('FileExplorerStore lifecycle', () => {
  it('expands a single-directory chain until it reaches a non-directory child', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(1);
    expect(store.snapshot('project-1').roots[0]?.children).toBeNull();

    await store.toggleDirectory('project-1', 'src', true);
    expect(api.listWorkspaceDirectory.mock.calls.map((call) => call[1])).toEqual(['', 'src', 'src/nested']);
    expect(store.snapshot('project-1').expanded).toEqual(new Set(['src', 'src/nested']));
  });

  it('keeps the directory display mode in the project tree lifecycle', () => {
    const store = createStore();
    expect(store.snapshot('project-1').displayMode).toBe('compact');

    store.setDisplayMode('project-1', 'tree');

    expect(store.snapshot('project-1').displayMode).toBe('tree');
  });

  it('ignores an obsolete search response and keeps only the latest request', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    let finishFirst!: (value: { requestId: string; entries: WorkspaceDirectoryEntryVm[]; truncated: boolean }) => void;
    api.searchWorkspaceFiles
      .mockImplementationOnce((_projectId: string, _query: string, requestId: string) => new Promise((resolve) => {
        finishFirst = resolve;
      }))
      .mockImplementationOnce(async (_projectId: string, _query: string, requestId: string) => ({
        requestId,
        entries: [file('latest.rs')],
        truncated: false,
      }));

    store.setSearchQuery('project-1', 'first');
    await vi.advanceTimersByTimeAsync(200);
    store.setSearchQuery('project-1', 'latest');
    await vi.advanceTimersByTimeAsync(200);
    finishFirst({ requestId: 'project-1:1', entries: [file('stale.rs')], truncated: false });
    await Promise.resolve();

    expect(store.snapshot('project-1').searchResult?.entries[0]?.name).toBe('latest.rs');
  });

  it('clears search and expands parent directories when revealing a result', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    store.setSearchQuery('project-1', 'main');
    await store.revealFile('project-1', 'src/nested/main.rs');

    const snapshot = store.snapshot('project-1');
    expect(snapshot.searchQuery).toBe('');
    expect(snapshot.expanded).toEqual(new Set(['src', 'src/nested']));
  });

  it('bounds project tree snapshots to the documented 24-project LRU', async () => {
    const store = createStore();
    await store.loadRoot('project-0');
    for (let index = 1; index <= 24; index += 1) store.snapshot(`project-${index}`);

    expect(store.snapshot('project-0').status).toBe('idle');
  });

  it('keeps content-only file changes outside the directory invalidation boundary', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    const roots = store.snapshot('project-1').roots;
    const event: WorkspaceFileChangedEventVm = {
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\README.md',
      kind: 'modified',
      revision: { byteLength: 20, modifiedAtNs: '2', contentHash: 'changed' },
      operationId: 'write-1',
    };

    store.applyFileChange(event);
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(store.snapshot('project-1').roots).toBe(roots);
    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(1);
  });

  it('keeps atomic-save rename events outside the directory invalidation boundary', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    const roots = store.snapshot('project-1').roots;

    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\README.md',
      kind: 'renamed',
      revision: { byteLength: 20, modifiedAtNs: '2', contentHash: 'saved' },
      operationId: 'write-1',
    });
    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\.README.md.a1B2c3',
      kind: 'renamed',
      revision: null,
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(store.snapshot('project-1').roots).toBe(roots);
    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(1);
  });

  it('also treats an external atomic replacement of a known path as content-only', async () => {
    const store = createStore();
    await store.loadRoot('project-1');

    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\README.md',
      kind: 'renamed',
      revision: { byteLength: 20, modifiedAtNs: '2', contentHash: 'external-save' },
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(1);
  });

  it('invalidates directory structure for create events', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\new.md',
      kind: 'created',
      revision: { byteLength: 0, modifiedAtNs: '2', contentHash: 'new' },
      operationId: null,
    });

    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(2);
  });

  it('refreshes root structure without replacing the mounted tree with a loading state', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    const beforeRefresh = store.snapshot('project-1');
    let completeRefresh!: (entries: WorkspaceDirectoryEntryVm[]) => void;
    api.listWorkspaceDirectory.mockImplementationOnce(() => new Promise((resolve) => {
      completeRefresh = resolve;
    }));

    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\new.md',
      kind: 'created',
      revision: { byteLength: 0, modifiedAtNs: '2', contentHash: 'new' },
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(store.snapshot('project-1').status).toBe('ready');
    expect(store.snapshot('project-1').roots).toBe(beforeRefresh.roots);

    completeRefresh([directory('src'), file('README.md'), file('new.md')]);
    await Promise.resolve();

    expect(store.snapshot('project-1').status).toBe('ready');
    expect(store.snapshot('project-1').roots.map((entry) => entry.name)).toEqual(['src', 'README.md', 'new.md']);
    expect(store.snapshot('project-1').treeScrollTop).toBe(beforeRefresh.treeScrollTop);
  });

  it('keeps already loaded descendants visible while an expanded tree reconciles', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    await store.toggleDirectory('project-1', 'src', true);
    const before = store.snapshot('project-1').roots;
    expect(fileTreeView(before, 'tree').flatMap((node) => node.children ?? []).flatMap((node) => node.children ?? []).map((node) => node.displayName)).toEqual(['main.rs']);

    const pending = new Map<string, (entries: WorkspaceDirectoryEntryVm[]) => void>();
    api.listWorkspaceDirectory.mockImplementation((_projectId: string, path: string) => {
      if (path === '') return Promise.resolve([directory('src'), file('README.md')]);
      return new Promise((resolve) => {
        pending.set(path, resolve);
      });
    });
    let droppedLoadedDescendant = false;
    const unsubscribe = store.subscribe(() => {
      const visible = fileTreeView(store.snapshot('project-1').roots, 'tree');
      const names = visible.flatMap((node) => [node.displayName, ...(node.children ?? []).flatMap((child) => [child.displayName, ...(child.children ?? []).map((nested) => nested.displayName)])]);
      if (!names.includes('main.rs')) droppedLoadedDescendant = true;
    });

    const reconciliation = store.reconcile('project-1');
    await vi.waitFor(() => expect(pending.has('src')).toBe(true));

    expect(droppedLoadedDescendant).toBe(false);
    expect(store.snapshot('project-1').roots).toBe(before);
    expect(store.snapshot('project-1').roots[0]?.loading).toBe(false);

    pending.get('src')!([directory('nested', 'src/nested'), file('added.rs', 'src/added.rs')]);
    await vi.waitFor(() => expect(pending.has('src/nested')).toBe(true));

    const src = store.snapshot('project-1').roots[0];
    expect(src?.children?.map((entry) => entry.name)).toEqual(['nested', 'added.rs']);
    expect(src?.children?.[0]?.children?.[0]?.name).toBe('main.rs');
    expect(droppedLoadedDescendant).toBe(false);

    pending.get('src/nested')!([file('main.rs', 'src/nested/main.rs')]);
    await reconciliation;
    unsubscribe();
    expect(droppedLoadedDescendant).toBe(false);
    expect(store.snapshot('project-1').expanded).toEqual(new Set(['src', 'src/nested']));
  });

  it('reconciles a cached tree on workspace reactivation without resetting ready UI state', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    store.setTreeScrollTop('project-1', 240);
    api.listWorkspaceDirectory.mockResolvedValueOnce([
      directory('src'),
      file('README.md'),
      directory('third-party-folder'),
    ]);

    const reconciliation = store.reconcile('project-1');

    expect(store.snapshot('project-1').status).toBe('ready');
    expect(store.snapshot('project-1').treeScrollTop).toBe(240);
    await reconciliation;
    expect(store.snapshot('project-1').roots.map((entry) => entry.name)).toEqual([
      'src',
      'README.md',
      'third-party-folder',
    ]);
    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(2);
  });

  it('invalidates directory structure when a known node is removed', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\README.md',
      kind: 'removed',
      revision: null,
      operationId: null,
    });

    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(2);
  });

  it('reruns an active filename search with the directory refresh and keeps the previous results visible', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    api.searchWorkspaceFiles.mockResolvedValueOnce({
      requestId: 'project-1:1',
      entries: [file('README.md')],
      truncated: false,
    });
    store.setSearchQuery('project-1', 'read');
    await vi.advanceTimersByTimeAsync(200);
    expect(store.snapshot('project-1').searchStatus).toBe('ready');

    let finishSearch!: (value: { requestId: string; entries: WorkspaceDirectoryEntryVm[]; truncated: boolean }) => void;
    api.searchWorkspaceFiles.mockImplementationOnce((_projectId: string, _query: string, requestId: string) => new Promise((resolve) => {
      finishSearch = resolve;
    }));
    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\notes.md',
      kind: 'created',
      revision: null,
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(store.snapshot('project-1').searchStatus).toBe('ready');
    expect(store.snapshot('project-1').searchResult?.entries.map((entry) => entry.name)).toEqual(['README.md']);
    finishSearch({ requestId: 'project-1:2', entries: [file('notes.md')], truncated: false });
    await vi.waitFor(() => expect(store.snapshot('project-1').searchResult?.entries.map((entry) => entry.name)).toEqual(['notes.md']));
    expect(api.searchWorkspaceFiles).toHaveBeenCalledTimes(2);
  });

  it('does not rerun filename search for a content-only change', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    api.searchWorkspaceFiles.mockResolvedValue({
      requestId: 'project-1:1',
      entries: [file('README.md')],
      truncated: false,
    });
    store.setSearchQuery('project-1', 'read');
    await vi.advanceTimersByTimeAsync(200);

    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\README.md',
      kind: 'modified',
      revision: { byteLength: 20, modifiedAtNs: '2', contentHash: 'changed' },
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    expect(api.searchWorkspaceFiles).toHaveBeenCalledTimes(1);
  });

  it('reruns the active search when the file panel reconciles after reactivation', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    api.searchWorkspaceFiles
      .mockResolvedValueOnce({ requestId: 'project-1:1', entries: [file('README.md')], truncated: false })
      .mockResolvedValueOnce({ requestId: 'project-1:2', entries: [file('notes.md')], truncated: false });
    store.setSearchQuery('project-1', 'read');
    await vi.advanceTimersByTimeAsync(200);

    await store.reconcile('project-1');

    expect(store.snapshot('project-1').searchStatus).toBe('ready');
    expect(store.snapshot('project-1').searchResult?.entries.map((entry) => entry.name)).toEqual(['notes.md']);
    expect(api.searchWorkspaceFiles).toHaveBeenCalledTimes(2);
  });

  it('rehydrates expanded descendants when a new file splits a compact chain', async () => {
    let splitChain = false;
    api.listWorkspaceDirectory.mockImplementation(async (_projectId: string, path: string) => {
      if (path === '') return [directory('src')];
      if (path === 'src') return [directory('nested', 'src/nested')];
      if (path === 'src/nested') {
        return splitChain
          ? [directory('deeper', 'src/nested/deeper'), file('new.md', 'src/nested/new.md')]
          : [directory('deeper', 'src/nested/deeper')];
      }
      return [file('main.rs', 'src/nested/deeper/main.rs')];
    });
    const store = createStore();
    await store.loadRoot('project-1');
    await store.toggleDirectory('project-1', 'src', true);
    splitChain = true;

    store.applyFileChange({
      projectId: 'project-1',
      canonicalPath: 'D:\\repo\\src\\nested\\new.md',
      kind: 'created',
      revision: { byteLength: 0, modifiedAtNs: '2', contentHash: 'new' },
      operationId: null,
    });
    await vi.advanceTimersByTimeAsync(FALLBACK_WORKSPACE_FILES.watchDebounceMs);

    const compactRoot = fileTreeView(store.snapshot('project-1').roots, 'compact')[0];
    expect(compactRoot?.displayName).toBe('src.nested');
    expect(compactRoot?.children?.map((entry) => entry.displayName)).toEqual(['deeper', 'new.md']);
    expect(compactRoot?.children?.[0]?.children?.[0]?.displayName).toBe('main.rs');
  });
});
