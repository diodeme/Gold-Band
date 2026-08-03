import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  listWorkspaceDirectory: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
}));

vi.mock('@/api', () => api);

import { FileExplorerStore } from '@/components/workspace/files/file-explorer-store';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import type { WorkspaceDirectoryEntryVm } from '@/types';

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
    path === '' ? [directory('src')] : path === 'src' ? [directory('nested', 'src/nested')] : [file('main.rs', 'src/nested/main.rs')]
  ));
});

afterEach(() => vi.useRealTimers());

describe('FileExplorerStore lifecycle', () => {
  it('loads one directory level at a time and restores expanded paths', async () => {
    const store = createStore();
    await store.loadRoot('project-1');
    expect(api.listWorkspaceDirectory).toHaveBeenCalledTimes(1);
    expect(store.snapshot('project-1').roots[0]?.children).toBeNull();

    await store.toggleDirectory('project-1', 'src', true);
    await store.toggleDirectory('project-1', 'src/nested', true);
    expect(api.listWorkspaceDirectory.mock.calls.map((call) => call[1])).toEqual(['', 'src', 'src/nested']);
    expect(store.snapshot('project-1').expanded).toEqual(new Set(['src', 'src/nested']));
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
});
