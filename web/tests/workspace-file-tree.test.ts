import { describe, expect, it } from 'vitest';
import {
  consumePendingTreeReveal,
  copyableAbsolutePath,
  shouldActivateTreeFile,
  treeOverscanCount,
  treeViewportContentHeight,
} from '@/components/workspace/files/WorkspaceFileTree';
import { FileExplorerStore } from '@/components/workspace/files/file-explorer-store';
import type { FileTreeNode } from '@/components/workspace/files/file-explorer-store';

const fileNode: FileTreeNode = {
  id: 'README.md',
  name: 'README.md',
  relativePath: 'README.md',
  canonicalPath: 'D:\\repo\\README.md',
  kind: 'file',
  hasChildren: false,
  byteLength: 10,
  modifiedAtNs: '1',
  children: [],
  loading: false,
};

describe('workspace file tree reveal lifecycle', () => {
  it('passes the virtual list the padding-free content-box height', () => {
    expect(treeViewportContentHeight(640, 6, 6)).toBe(628);
    expect(treeViewportContentHeight(12, 6, 6)).toBe(1);
  });

  it('renders at least two viewportfuls around fast virtual-tree scrolling', () => {
    expect(treeOverscanCount(480)).toBe(30);
    expect(treeOverscanCount(160)).toBe(24);
    expect(treeOverscanCount(4_000)).toBe(96);
  });
  it('consumes selection reveal once so later directory snapshots do not scroll again', () => {
    const first = consumePendingTreeReveal('d:/REPO/readme.md', [fileNode]);
    expect(first).toEqual({ pendingPath: null, targetId: 'README.md' });

    const afterDirectoryExpansion = consumePendingTreeReveal(first.pendingPath, [{ ...fileNode }]);
    expect(afterDirectoryExpansion).toEqual({ pendingPath: null, targetId: null });
  });

  it('reveals a selection only when its identity changes across tree remounts', () => {
    const store = new FileExplorerStore();
    expect(store.takeSelectionReveal('project-1', 'D:\\repo\\README.md')).toBe(true);
    expect(store.takeSelectionReveal('project-1', 'd:/REPO/README.md')).toBe(false);
    expect(store.takeSelectionReveal('project-1', 'D:\\repo\\src\\main.rs')).toBe(true);
    expect(store.takeSelectionReveal('project-1', null)).toBe(false);
    expect(store.takeSelectionReveal('project-1', 'D:\\repo\\src\\main.rs')).toBe(true);
  });

  it('keeps a reveal pending until its lazy parent directories are loaded', () => {
    const beforeLoad = consumePendingTreeReveal('D:\\repo\\README.md', []);
    expect(beforeLoad.targetId).toBeNull();
    expect(consumePendingTreeReveal(beforeLoad.pendingPath, [fileNode]).targetId).toBe('README.md');
  });
});

describe('workspace file tree path actions', () => {
  it('copies normal Windows paths without the extended-length prefix', () => {
    expect(copyableAbsolutePath('\\\\?\\D:\\repo\\README.md')).toBe('D:\\repo\\README.md');
    expect(copyableAbsolutePath('\\\\?\\UNC\\server\\share\\README.md')).toBe('\\\\server\\share\\README.md');
    expect(copyableAbsolutePath('D:\\repo\\README.md')).toBe('D:\\repo\\README.md');
  });

  it('blocks file activation during a context-menu copy lifecycle', () => {
    expect(shouldActivateTreeFile(true, true)).toBe(false);
    expect(shouldActivateTreeFile(false, true)).toBe(false);
    expect(shouldActivateTreeFile(false, false)).toBe(true);
  });
});
