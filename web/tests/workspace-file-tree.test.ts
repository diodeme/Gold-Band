import { describe, expect, it } from 'vitest';
import {
  consumePendingTreeReveal,
  copyableAbsolutePath,
  copyableRelativePath,
  fileTreeDisplayModeToggle,
  shouldActivateTreeFile,
  treeOverscanCount,
  treeOpenState,
  treeRowMinimumWidth,
  treeRowOverflowStyle,
  treeViewportContentHeight,
} from '@/components/workspace/files/WorkspaceFileTree';
import { FileExplorerStore, fileTreeView } from '@/components/workspace/files/file-explorer-store';
import type { FileTreeNode, FileTreeViewNode } from '@/components/workspace/files/file-explorer-store';

const fileNode: FileTreeViewNode = {
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
  displayName: 'README.md',
};

const directoryNode = (name: string, relativePath: string, children: FileTreeNode[] | null): FileTreeNode => ({
  id: relativePath,
  name,
  relativePath,
  canonicalPath: `D:\\repo\\${relativePath.replaceAll('/', '\\')}`,
  kind: 'directory',
  hasChildren: true,
  byteLength: null,
  modifiedAtNs: '1',
  children,
  loading: false,
});

describe('workspace file tree reveal lifecycle', () => {
  it('shows the current display mode icon while labeling the target mode action', () => {
    expect(fileTreeDisplayModeToggle('compact')).toEqual({
      currentIcon: 'compact',
      targetMode: 'tree',
      labelKey: 'workspace.filesPanel.switchToTreeView',
    });
    expect(fileTreeDisplayModeToggle('tree')).toEqual({
      currentIcon: 'tree',
      targetMode: 'compact',
      labelKey: 'workspace.filesPanel.switchToCompactView',
    });
  });

  it('passes the virtual list the padding-free content-box height', () => {
    expect(treeViewportContentHeight(640, 6, 6)).toBe(628);
    expect(treeViewportContentHeight(12, 6, 6)).toBe(1);
  });

  it('renders at least two viewportfuls around fast virtual-tree scrolling', () => {
    expect(treeOverscanCount(480)).toBe(30);
    expect(treeOverscanCount(160)).toBe(24);
    expect(treeOverscanCount(4_000)).toBe(96);
  });

  it('derives overflow width from tree depth and compact content width', () => {
    expect(treeRowMinimumWidth(0)).toBe(190);
    expect(treeRowMinimumWidth(8)).toBe(302);
    expect(treeRowOverflowStyle('tree', 8)).toEqual({ minWidth: 302 });
    expect(treeRowOverflowStyle('compact', 8)).toEqual({
      width: 'max-content',
      minWidth: 'max(100%, 302px)',
    });
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

describe('workspace compact folder projection', () => {
  const mainFile: FileTreeNode = {
    ...fileNode,
    id: 'com/webank/qps/Main.java',
    name: 'Main.java',
    relativePath: 'com/webank/qps/Main.java',
    canonicalPath: 'D:\\repo\\com\\webank\\qps\\Main.java',
  };
  const qps = directoryNode('qps', 'com/webank/qps', [mainFile]);
  const webank = directoryNode('webank', 'com/webank', [qps]);
  const com = directoryNode('com', 'com', [webank]);

  it('merges a single-directory chain while keeping the chain-tail action path', () => {
    const compact = fileTreeView([com], 'compact');

    expect(compact[0]).toMatchObject({
      id: 'com',
      displayName: 'com.webank.qps',
      relativePath: 'com/webank/qps',
      canonicalPath: 'D:\\repo\\com\\webank\\qps',
    });
    expect(compact[0]?.children?.[0]?.displayName).toBe('Main.java');
    expect(treeOpenState(compact, new Set(['com/webank/qps']))).toMatchObject({ com: true });
  });

  it('keeps every real directory visible in tree mode', () => {
    const tree = fileTreeView([com], 'tree');

    expect(tree[0]?.displayName).toBe('com');
    expect(tree[0]?.children?.[0]?.displayName).toBe('webank');
    expect(tree[0]?.children?.[0]?.children?.[0]?.displayName).toBe('qps');
  });

  it('splits only the affected compact segment when a file appears in the middle', () => {
    const readme: FileTreeNode = {
      ...fileNode,
      id: 'com/webank/README.md',
      relativePath: 'com/webank/README.md',
      canonicalPath: 'D:\\repo\\com\\webank\\README.md',
    };
    const changed = directoryNode('com', 'com', [
      directoryNode('webank', 'com/webank', [qps, readme]),
    ]);

    const compact = fileTreeView([changed], 'compact');

    expect(compact[0]?.displayName).toBe('com.webank');
    expect(compact[0]?.children?.map((entry) => entry.displayName)).toEqual(['qps', 'README.md']);
  });
});

describe('workspace file tree path actions', () => {
  it('copies normal Windows paths without the extended-length prefix', () => {
    expect(copyableAbsolutePath('\\\\?\\D:\\repo\\README.md')).toBe('D:\\repo\\README.md');
    expect(copyableAbsolutePath('\\\\?\\UNC\\server\\share\\README.md')).toBe('\\\\server\\share\\README.md');
    expect(copyableAbsolutePath('D:\\repo\\README.md')).toBe('D:\\repo\\README.md');
  });

  it('uses portable separators for relative paths in every directory context menu', () => {
    expect(copyableRelativePath('rounds\\round-001\\node.json')).toBe('rounds/round-001/node.json');
  });

  it('blocks file activation during a context-menu copy lifecycle', () => {
    expect(shouldActivateTreeFile(true, true)).toBe(false);
    expect(shouldActivateTreeFile(false, true)).toBe(false);
    expect(shouldActivateTreeFile(false, false)).toBe(true);
  });
});
