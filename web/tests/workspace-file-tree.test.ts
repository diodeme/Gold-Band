import { describe, expect, it } from 'vitest';
import {
  consumePendingTreeReveal,
  copyableAbsolutePath,
  shouldActivateTreeFile,
} from '@/components/workspace/files/WorkspaceFileTree';
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
  it('consumes selection reveal once so later directory snapshots do not scroll again', () => {
    const first = consumePendingTreeReveal('d:/REPO/readme.md', [fileNode]);
    expect(first).toEqual({ pendingPath: null, targetId: 'README.md' });

    const afterDirectoryExpansion = consumePendingTreeReveal(first.pendingPath, [{ ...fileNode }]);
    expect(afterDirectoryExpansion).toEqual({ pendingPath: null, targetId: null });
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
