import { describe, expect, it } from 'vitest';
import {
  createInitialRightWorkspaceState,
  fileWorkspaceResourceKey,
  rightWorkspaceReducer,
  type FileWorkspaceResource,
} from '@/components/workspace/right-workspace-context';

function fileResource(targetLine: number, targetRevision: number): FileWorkspaceResource {
  const canonicalPath = 'D:\\Repo\\src\\client.rs';
  return {
    kind: 'file',
    key: fileWorkspaceResourceKey('project-1', canonicalPath),
    scopeKey: 'draft:project-1',
    projectId: 'project-1',
    title: 'client.rs',
    attention: false,
    locator: { projectId: 'project-1', canonicalPath, relativePath: 'src/client.rs', scope: 'workspace' },
    target: { line: targetLine, column: null, endLine: null },
    targetRevision,
  };
}

function anotherFileResource(): FileWorkspaceResource {
  const canonicalPath = 'D:\\Repo\\src\\other.rs';
  return {
    ...fileResource(1, 1),
    key: fileWorkspaceResourceKey('project-1', canonicalPath),
    title: 'other.rs',
    locator: { projectId: 'project-1', canonicalPath, relativePath: 'src/other.rs', scope: 'workspace' },
  };
}

describe('right workspace file resources', () => {
  it('uses stable case-insensitive keys for Windows drive paths', () => {
    expect(fileWorkspaceResourceKey('project-1', 'D:\\Repo\\SRC\\client.rs'))
      .toBe(fileWorkspaceResourceKey('project-1', 'd:/repo/src/CLIENT.rs'));
  });

  it('keeps file navigation inside one Files tab and updates the target revision', () => {
    const first = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: fileResource(2727, 1) });
    const second = rightWorkspaceReducer(first, { type: 'open', resource: fileResource(3302, 2) });

    expect(second.tabs).toHaveLength(1);
    expect(second.tabs[0]?.kind).toBe('file-browser');
    expect(second.tabs[0]?.kind === 'file-browser' && second.tabs[0].selectedFile?.target?.line).toBe(3302);
    expect(second.tabs[0]?.kind === 'file-browser' && second.tabs[0].selectedFile?.targetRevision).toBe(2);
  });

  it('replaces the selected file instead of creating a second Files tab', () => {
    const first = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: fileResource(10, 1) });
    const second = rightWorkspaceReducer(first, { type: 'open', resource: anotherFileResource() });

    expect(second.tabs).toHaveLength(1);
    expect(second.tabs[0]?.kind === 'file-browser' && second.tabs[0].selectedFile?.title).toBe('other.rs');
  });
});
