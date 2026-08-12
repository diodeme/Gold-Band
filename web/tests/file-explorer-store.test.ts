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
