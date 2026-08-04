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

describe('right workspace file resources', () => {
  it('uses stable case-insensitive keys for Windows drive paths', () => {
    expect(fileWorkspaceResourceKey('project-1', 'D:\\Repo\\SRC\\client.rs'))
      .toBe(fileWorkspaceResourceKey('project-1', 'd:/repo/src/CLIENT.rs'));
  });

  it('reuses the same tab and updates the target revision for another line', () => {
    const first = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: fileResource(2727, 1) });
    const second = rightWorkspaceReducer(first, { type: 'open', resource: fileResource(3302, 2) });

    expect(second.tabs).toHaveLength(1);
    expect(second.tabs[0]?.kind === 'file' && second.tabs[0].target?.line).toBe(3302);
    expect(second.tabs[0]?.kind === 'file' && second.tabs[0].targetRevision).toBe(2);
  });
});
