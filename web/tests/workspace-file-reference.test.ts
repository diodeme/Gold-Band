import { beforeEach, describe, expect, it, vi } from 'vitest';

import { resolveWorkspaceFileLink } from '@/api';
import { fileWorkspaceResourceKey } from '@/components/workspace/right-workspace-context';
import { openWorkspaceFileReference } from '@/lib/workspace-file-reference';

vi.mock('@/api', () => ({
  resolveWorkspaceFileLink: vi.fn(),
}));

describe('workspace file reference opening', () => {
  beforeEach(() => {
    vi.mocked(resolveWorkspaceFileLink).mockReset();
  });

  it('resolves a lightweight reference before creating a workspace file resource', async () => {
    vi.mocked(resolveWorkspaceFileLink).mockResolvedValue({
      locator: {
        projectId: 'project-1',
        canonicalPath: 'D:/workspace/src/a.ts',
        relativePath: 'src/a.ts',
        scope: 'workspace',
      },
      target: null,
      externalAccessGrant: null,
    });
    const openResource = vi.fn().mockResolvedValue(undefined);

    await openWorkspaceFileReference({
      projectId: 'project-1',
      relativePath: 'src/a.ts',
      name: 'a.ts',
    }, 'scope-1', openResource);

    expect(resolveWorkspaceFileLink).toHaveBeenCalledWith('project-1', 'src/a.ts');
    expect(openResource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'file-browser',
      key: 'file-browser:project-1',
      selectedFile: expect.objectContaining({
        key: fileWorkspaceResourceKey('project-1', 'D:/workspace/src/a.ts'),
      }),
    }));
  });

  it('does not create a file resource when resolution fails', async () => {
    vi.mocked(resolveWorkspaceFileLink).mockRejectedValue({
      code: 'workspace-file.not-found',
      params: { path: 'src/a.ts' },
    });
    const openResource = vi.fn();

    await expect(openWorkspaceFileReference({
      projectId: 'project-1',
      relativePath: 'src/a.ts',
    }, 'scope-1', openResource)).rejects.toMatchObject({
      code: 'workspace-file.not-found',
    });

    expect(openResource).not.toHaveBeenCalled();
  });
});
