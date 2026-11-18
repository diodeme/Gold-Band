import { beforeEach, describe, expect, it, vi } from 'vitest';

import { resolveRightWorkspaceFileTarget, resolveWorkspaceFileLink } from '@/api';
import { createInitialRightWorkspaceState, fileWorkspaceResourceKey, projectWorkspaceTabRoots, rightWorkspaceReducer, type RightWorkspaceResource } from '@/components/workspace/right-workspace-context';
import type { PromptWorkspaceFileRef } from '@/types';
import { composerWorkspaceFileRefFromLocator, openWorkspaceFileReference } from '@/lib/workspace-file-reference';
import { createUserPromptSubmission } from '@/lib/composer-context';
import { fileReferenceIdentity } from '@/lib/file-reference-source';

vi.mock('@/api', () => ({
  resolveWorkspaceFileLink: vi.fn(),
  resolveRightWorkspaceFileTarget: vi.fn(),
}));

describe('workspace file reference opening', () => {
  beforeEach(() => {
    vi.mocked(resolveWorkspaceFileLink).mockReset();
    vi.mocked(resolveRightWorkspaceFileTarget).mockReset();
  });

  const fixedSources: Extract<PromptWorkspaceFileRef, { kind: 'git-file' | 'turn-file' }>[] = [
    { kind: 'git-file', projectId: 'project-1', workspacePath: 'D:/source', relativePath: 'src/a.ts' },
    { kind: 'turn-file', locator: { projectId: 'project-1', taskId: 'task-source', taskUuid: 'uuid-source',
      runId: 'run-1', roundId: 'round-1', nodeId: 'node-1', attemptId: 'attempt-1', branchId: 'main' },
      changeSetId: 'set-1', changeId: 'change-1' },
  ];
  it.each(fixedSources)('keeps the $kind source through preview-reference conversion and submission', (source) => {
    const locator = { projectId: 'project-1', canonicalPath: 'D:/source/src/a.ts', relativePath: 'src/a.ts', scope: 'workspace' as const };
    const ref = composerWorkspaceFileRefFromLocator(locator, source)!;
    expect(createUserPromptSubmission('', [], null, [ref]).workspaceFiles).toEqual([source]);
    const workspaceRef = composerWorkspaceFileRefFromLocator(locator)!;
    expect(ref.id).not.toBe(workspaceRef.id);
  });

  it('preserves ordinary workspace references and rejects external locators without a fixed source', () => {
    const locator = { projectId: 'project-1', canonicalPath: 'D:/workspace/src/a.ts', relativePath: 'src\\a.ts', scope: 'workspace' as const };
    const ref = composerWorkspaceFileRefFromLocator(locator)!;
    expect(createUserPromptSubmission('', [], null, [ref]).workspaceFiles).toEqual([
      { kind: 'workspace', projectId: 'project-1', relativePath: 'src/a.ts' },
    ]);
    expect(composerWorkspaceFileRefFromLocator({ ...locator, scope: 'external' })).toBeNull();
    expect(composerWorkspaceFileRefFromLocator({ ...locator, scope: 'external', relativePath: null })).toBeNull();
  });

  const attachmentSource: Extract<PromptWorkspaceFileRef, { kind: 'turn-attachment' }> = {
    kind: 'turn-attachment',
    locator: { projectId: 'project-1', taskId: 'task-source', taskUuid: 'uuid-source',
      runId: 'run-1', roundId: 'round-1', nodeId: 'node-1', attemptId: 'attempt-1',
      branchId: 'main', outerNodeId: 'outer-node', outerAttemptId: 'outer-attempt' },
    changeSetId: 'set-1', attachmentId: 'attachment-1',
  };

  it.each([
    'D:/runtime/attempt/attachments/reports/report.md',
    'D:\\runtime\\attempt\\attachments\\reports\\report.md',
  ])('preserves a controlled external attachment through opening, referencing and submission: %s', async (canonicalPath) => {
    const locator = { projectId: 'project-1', canonicalPath, relativePath: null, scope: 'external' as const };
    vi.mocked(resolveRightWorkspaceFileTarget).mockResolvedValue({
      source: attachmentSource, projectId: 'project-1', name: 'report.md', absolutePath: canonicalPath,
      relativePath: 'reports/report.md', relativeBase: 'attachment', workspacePath: null,
      byteLength: 15, mimeType: 'text/markdown', availability: { kind: 'available' },
    });
    vi.mocked(resolveWorkspaceFileLink).mockResolvedValue({ locator, target: null, externalAccessGrant: null });
    const open = vi.fn();
    await openWorkspaceFileReference({ source: attachmentSource, projectId: 'project-1', relativePath: 'reports/report.md' }, 'scope', open, null);
    const selectedFile = (open.mock.calls[0][0] as Extract<RightWorkspaceResource, { kind: 'file-browser' }>).selectedFile!;

    const ref = composerWorkspaceFileRefFromLocator(selectedFile.locator, selectedFile.source);

    expect(ref).toMatchObject({ id: fileReferenceIdentity(attachmentSource), source: attachmentSource,
      projectId: 'project-1', name: 'report.md', relativePath: 'report.md', mimeType: 'text/markdown', canonicalPath });
    expect(createUserPromptSubmission('', [], null, [ref!]).workspaceFiles).toEqual([attachmentSource]);
  });
  it.each(fixedSources)('opens a $kind from its own root when the receiving worktree is unavailable', async (source) => {
    const locator = { projectId: 'project-1', canonicalPath: 'D:/source/src/a.ts', relativePath: 'src/a.ts', scope: 'workspace' as const };
    vi.mocked(resolveRightWorkspaceFileTarget).mockResolvedValue({ source, projectId: 'project-1', name: 'a.ts',
      absolutePath: locator.canonicalPath, relativePath: locator.relativePath, relativeBase: 'workspace',
      workspacePath: 'D:/source', byteLength: 1, mimeType: 'text/plain', availability: { kind: 'available' } });
    vi.mocked(resolveWorkspaceFileLink).mockResolvedValue({ locator, target: null, externalAccessGrant: null });
    let state = createInitialRightWorkspaceState();
    const open = vi.fn((resource: RightWorkspaceResource) => { state = rightWorkspaceReducer(state, { type: 'open', resource }); });
    await openWorkspaceFileReference({ source, projectId: 'project-1', relativePath: 'src/a.ts' }, 'scope', open, null);
    expect(resolveWorkspaceFileLink).toHaveBeenCalledWith({ projectId: 'project-1', workspacePath: 'D:/source' }, locator.canonicalPath);
    const released = { kind: 'unavailable', reason: 'released', workspacePath: 'D:/receiver' } as const;
    state = projectWorkspaceTabRoots(state, released);
    expect(state.tabs[0]).toMatchObject({ root: released, browseMain: false, selectedFile: { locator, workspacePath: 'D:/source', source } });
    state = projectWorkspaceTabRoots(state, { kind: 'available', workspacePath: 'D:/receiver-restored' });
    expect(state.tabs[0]).toMatchObject({ root: { workspacePath: 'D:/receiver-restored' }, selectedFile: { locator, workspacePath: 'D:/source', source } });
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
    }, 'scope-1', openResource, { projectId: 'project-1', workspacePath: 'D:/workspace/.wt/a' });

    expect(resolveWorkspaceFileLink).toHaveBeenCalledWith(
      { projectId: 'project-1', workspacePath: 'D:/workspace/.wt/a' },
      'src/a.ts',
    );
    expect(openResource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'file-browser',
      key: 'file-browser:project-1',
      selectedFile: expect.objectContaining({
        key: fileWorkspaceResourceKey('project-1', 'D:/workspace/src/a.ts'),
        workspacePath: 'D:/workspace/.wt/a',
      }),
    }));
  });

  it('resolves a reference of another project against that project root', async () => {
    vi.mocked(resolveWorkspaceFileLink).mockResolvedValue({
      locator: { projectId: 'project-2', canonicalPath: 'D:/other/a.ts', relativePath: 'a.ts', scope: 'workspace' },
      target: null,
      externalAccessGrant: null,
    });

    await openWorkspaceFileReference(
      { projectId: 'project-2', relativePath: 'a.ts' },
      'scope-1',
      vi.fn().mockResolvedValue(undefined),
      { projectId: 'project-1', workspacePath: 'D:/workspace/.wt/a' },
    );

    expect(resolveWorkspaceFileLink).toHaveBeenCalledWith({ projectId: 'project-2', workspacePath: null }, 'a.ts');
  });

  it('opens the files tab without reading the project root while the worktree is unavailable', async () => {
    const openResource = vi.fn().mockResolvedValue(undefined);

    await openWorkspaceFileReference({ projectId: 'project-1', relativePath: 'src/a.ts' }, 'scope-1', openResource, null);

    expect(resolveWorkspaceFileLink).not.toHaveBeenCalled();
    expect(openResource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'file-browser',
      key: 'file-browser:project-1',
      selectedFile: null,
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
    }, 'scope-1', openResource, { projectId: 'project-1', workspacePath: null })).rejects.toMatchObject({
      code: 'workspace-file.not-found',
    });

    expect(openResource).not.toHaveBeenCalled();
  });
});
