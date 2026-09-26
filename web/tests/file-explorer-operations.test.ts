import { beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  listWorkspaceDirectory: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
  createWorkspaceEntry: vi.fn(),
  renameWorkspaceEntry: vi.fn(),
  deleteWorkspaceEntry: vi.fn(),
  restoreWorkspaceEntry: vi.fn(),
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

import {
  FILE_TREE_DRAFT_ID,
  FileExplorerStore,
  fileTreeView,
  fileTreeWithDraft,
  type FileTreeEntryMutation,
} from '@/components/workspace/files/file-explorer-store';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import type { WorkspaceDirectoryEntryVm } from '@/types';

const PROJECT = 'project-1';

const entry = (relativePath: string, kind: 'file' | 'directory'): WorkspaceDirectoryEntryVm => ({
  name: relativePath.slice(relativePath.lastIndexOf('/') + 1),
  relativePath,
  canonicalPath: `D:\\repo\\${relativePath.replaceAll('/', '\\')}`,
  kind,
  hasChildren: kind === 'directory',
  byteLength: kind === 'file' ? 0 : null,
  modifiedAtNs: '1',
});

let listing: Record<string, WorkspaceDirectoryEntryVm[]>;

function createStore() {
  const store = new FileExplorerStore();
  store.configure(FALLBACK_WORKSPACE_FILES);
  return store;
}

async function readyStore() {
  const store = createStore();
  await store.loadRoot(PROJECT);
  return store;
}

function recordMutations(store: FileExplorerStore) {
  const mutations: FileTreeEntryMutation[] = [];
  store.subscribeEntryMutations((mutation) => mutations.push(mutation));
  return mutations;
}

beforeEach(() => {
  vi.clearAllMocks();
  listing = {
    '': [entry('src', 'directory'), entry('README.md', 'file')],
    src: [entry('src/lib', 'directory'), entry('src/main.rs', 'file')],
    'src/lib': [entry('src/lib/mod.rs', 'file')],
  };
  api.listWorkspaceDirectory.mockImplementation(async (_projectId: string, path: string) => listing[path] ?? []);
});

describe('file tree draft rows', () => {
  it('places a file draft after folders and a folder draft first', async () => {
    const store = await readyStore();
    const roots = store.snapshot(PROJECT).roots;

    expect(fileTreeWithDraft(roots, { parentRelativePath: '', kind: 'file' }).map((node) => node.id))
      .toEqual(['src', FILE_TREE_DRAFT_ID, 'README.md']);
    expect(fileTreeWithDraft(roots, { parentRelativePath: '', kind: 'directory' }).map((node) => node.id))
      .toEqual([FILE_TREE_DRAFT_ID, 'src', 'README.md']);
  });

  it('expands and loads the parent folder before showing the draft inside it', async () => {
    const store = await readyStore();

    await store.startDraft(PROJECT, 'src', 'file');

    const snapshot = store.snapshot(PROJECT);
    expect(snapshot.expanded.has('src')).toBe(true);
    expect(snapshot.draft).toEqual({ parentRelativePath: 'src', kind: 'file' });
    const src = fileTreeWithDraft(snapshot.roots, snapshot.draft).find((node) => node.id === 'src');
    expect(src?.children?.map((node) => node.id)).toEqual(['src/lib', FILE_TREE_DRAFT_ID, 'src/main.rs']);
  });

  it('never folds a draft folder into a compact folder chain', async () => {
    listing.empty = [];
    listing[''] = [entry('empty', 'directory')];
    const store = await readyStore();
    await store.startDraft(PROJECT, 'empty', 'directory');

    const snapshot = store.snapshot(PROJECT);
    const view = fileTreeView(fileTreeWithDraft(snapshot.roots, snapshot.draft), 'compact');

    expect(view[0]?.displayName).toBe('empty');
    expect(view[0]?.children?.map((node) => node.id)).toEqual([FILE_TREE_DRAFT_ID]);
  });
});

describe('file tree operations and undo', () => {
  it('creates an entry from the draft, then undo moves it to the trash', async () => {
    const store = await readyStore();
    const mutations = recordMutations(store);
    const created = entry('src/new.ts', 'file');
    api.createWorkspaceEntry.mockResolvedValue(created);
    api.deleteWorkspaceEntry.mockResolvedValue({ receiptId: 'r-1', entry: created });

    await store.startDraft(PROJECT, 'src', 'file');
    const result = await store.createEntry(PROJECT, 'new.ts');

    expect(result).toEqual({ status: 'done', entry: created });
    expect(api.createWorkspaceEntry).toHaveBeenCalledWith({
      projectId: PROJECT,
      parentRelativePath: 'src',
      name: 'new.ts',
      kind: 'file',
    });
    expect(store.snapshot(PROJECT).draft).toBeNull();
    expect(store.snapshot(PROJECT).pendingPath).toBeNull();
    expect(api.listWorkspaceDirectory).toHaveBeenLastCalledWith(PROJECT, 'src');

    expect(await store.undo(PROJECT)).toEqual({ status: 'done', entry: null });
    expect(api.deleteWorkspaceEntry).toHaveBeenCalledWith(PROJECT, 'src/new.ts');
    expect(mutations).toEqual([{ projectId: PROJECT, kind: 'removed', entry: created }]);
    expect(store.canUndo(PROJECT)).toBe(false);
  });

  it('renames a folder, carries its expanded descendants, and undo restores the original name', async () => {
    const store = await readyStore();
    await store.toggleDirectory(PROJECT, 'src', true);
    await store.toggleDirectory(PROJECT, 'src/lib', true);
    const mutations = recordMutations(store);
    const from = store.snapshot(PROJECT).roots[0]!;
    const to = entry('source', 'directory');
    api.renameWorkspaceEntry.mockResolvedValueOnce(to).mockResolvedValueOnce(entry('src', 'directory'));

    await store.renameEntry(PROJECT, from, 'source');

    expect(api.renameWorkspaceEntry).toHaveBeenCalledWith(PROJECT, 'src', 'source');
    expect(store.snapshot(PROJECT).expanded).toEqual(new Set(['source', 'source/lib']));
    expect(mutations[0]).toMatchObject({ kind: 'moved', from: { relativePath: 'src' }, to });

    await store.undo(PROJECT);

    expect(api.renameWorkspaceEntry).toHaveBeenLastCalledWith(PROJECT, 'source', 'src');
    expect(store.snapshot(PROJECT).expanded).toEqual(new Set(['src', 'src/lib']));
  });

  it('skips a rename that keeps the same name', async () => {
    const store = await readyStore();
    const readme = store.snapshot(PROJECT).roots[1]!;

    expect(await store.renameEntry(PROJECT, readme, 'README.md')).toEqual({ status: 'skipped' });
    expect(api.renameWorkspaceEntry).not.toHaveBeenCalled();
    expect(store.canUndo(PROJECT)).toBe(false);
  });

  it('deletes to the trash, forgets expanded descendants, and undo restores by receipt', async () => {
    const store = await readyStore();
    await store.toggleDirectory(PROJECT, 'src', true);
    const mutations = recordMutations(store);
    const src = store.snapshot(PROJECT).roots[0]!;
    api.deleteWorkspaceEntry.mockResolvedValue({ receiptId: 'receipt-7', entry: entry('src', 'directory') });
    api.restoreWorkspaceEntry.mockResolvedValue(entry('src', 'directory'));

    await store.deleteEntry(PROJECT, src);

    expect(store.snapshot(PROJECT).expanded.size).toBe(0);
    expect(mutations).toEqual([{ projectId: PROJECT, kind: 'removed', entry: entry('src', 'directory') }]);
    await store.undo(PROJECT);
    expect(api.restoreWorkspaceEntry).toHaveBeenCalledWith(PROJECT, 'receipt-7');
  });

  it('returns the structured backend error and records nothing when an operation fails', async () => {
    const store = await readyStore();
    api.createWorkspaceEntry.mockRejectedValue({ code: 'workspace-file.already-exists', params: { path: 'D:\\repo\\README.md' } });

    await store.startDraft(PROJECT, '', 'file');
    const result = await store.createEntry(PROJECT, 'README.md');

    expect(result).toEqual({
      status: 'failed',
      errorCode: 'workspace-file.already-exists',
      params: { path: 'D:\\repo\\README.md' },
    });
    expect(store.snapshot(PROJECT).draft).toBeNull();
    expect(store.canUndo(PROJECT)).toBe(false);
  });

  it('drops an undo step whose inverse fails instead of retrying it forever', async () => {
    const store = await readyStore();
    const src = store.snapshot(PROJECT).roots[0]!;
    api.deleteWorkspaceEntry.mockResolvedValue({ receiptId: 'gone', entry: src });
    api.restoreWorkspaceEntry.mockRejectedValue({ code: 'workspace-file.restore-unavailable', params: { reason: 'not-in-trash' } });
    await store.deleteEntry(PROJECT, src);

    const result = await store.undo(PROJECT);

    expect(result).toMatchObject({ status: 'failed', errorCode: 'workspace-file.restore-unavailable' });
    expect(store.canUndo(PROJECT)).toBe(false);
    expect(await store.undo(PROJECT)).toEqual({ status: 'skipped' });
  });

  it('rejects a second write while one is pending and exposes the pending entry', async () => {
    const store = await readyStore();
    const [src, readme] = store.snapshot(PROJECT).roots;
    let finishRename!: (value: WorkspaceDirectoryEntryVm) => void;
    api.renameWorkspaceEntry.mockImplementation(() => new Promise((resolve) => { finishRename = resolve; }));

    const first = store.renameEntry(PROJECT, readme!, 'NOTES.md');
    await vi.waitFor(() => expect(api.renameWorkspaceEntry).toHaveBeenCalled());
    expect(store.snapshot(PROJECT).pendingPath).toBe('README.md');
    expect(await store.deleteEntry(PROJECT, src!)).toEqual({ status: 'skipped' });
    expect(await store.undo(PROJECT)).toEqual({ status: 'skipped' });

    finishRename(entry('NOTES.md', 'file'));
    await first;
    expect(api.deleteWorkspaceEntry).not.toHaveBeenCalled();
    expect(store.snapshot(PROJECT).pendingPath).toBeNull();
  });

  it('keeps only the most recent 20 operations for undo', async () => {
    const store = await readyStore();
    api.createWorkspaceEntry.mockImplementation(async ({ name }: { name: string }) => entry(name, 'file'));
    for (let index = 0; index < 21; index += 1) {
      await store.startDraft(PROJECT, '', 'file');
      await store.createEntry(PROJECT, `file-${index}.txt`);
    }
    api.deleteWorkspaceEntry.mockImplementation(async (_projectId: string, path: string) => ({ receiptId: path, entry: entry(path, 'file') }));

    let undone = 0;
    while (store.canUndo(PROJECT)) {
      await store.undo(PROJECT);
      undone += 1;
    }

    expect(undone).toBe(20);
    expect(api.deleteWorkspaceEntry).not.toHaveBeenCalledWith(PROJECT, 'file-0.txt');
  });
});
