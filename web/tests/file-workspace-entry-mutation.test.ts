import { describe, expect, it } from 'vitest';
import { selectedFileAfterEntryMutation } from '@/components/workspace/files/FileWorkspacePanel';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileLocatorVm } from '@/types';

const entry = (relativePath: string, kind: 'file' | 'directory'): WorkspaceDirectoryEntryVm => ({
  name: relativePath.slice(relativePath.lastIndexOf('/') + 1),
  relativePath,
  canonicalPath: `D:\\repo\\${relativePath.replaceAll('/', '\\')}`,
  kind,
  hasChildren: false,
  byteLength: null,
  modifiedAtNs: null,
});

const locator: WorkspaceFileLocatorVm = {
  projectId: 'project-1',
  canonicalPath: 'D:\\repo\\src\\lib\\mod.rs',
  relativePath: 'src/lib/mod.rs',
  scope: 'workspace',
};

describe('open file after a tree mutation', () => {
  it('follows a renamed ancestor folder to the new path', () => {
    const next = selectedFileAfterEntryMutation(locator, {
      projectId: 'project-1',
      kind: 'moved',
      from: entry('src', 'directory'),
      to: entry('source', 'directory'),
    });
    expect(next).toMatchObject({
      name: 'mod.rs',
      relativePath: 'source/lib/mod.rs',
      canonicalPath: 'D:\\repo\\source\\lib\\mod.rs',
      kind: 'file',
    });
  });

  it('clears the selection when the file or an ancestor is deleted', () => {
    expect(selectedFileAfterEntryMutation(locator, { projectId: 'project-1', kind: 'removed', entry: entry('src/lib', 'directory') })).toBeNull();
    expect(selectedFileAfterEntryMutation(locator, { projectId: 'project-1', kind: 'removed', entry: entry('src/lib/mod.rs', 'file') })).toBeNull();
  });

  it('ignores mutations of unrelated siblings with a shared name prefix', () => {
    expect(selectedFileAfterEntryMutation(locator, { projectId: 'project-1', kind: 'removed', entry: entry('src/li', 'directory') })).toBeUndefined();
  });
});
