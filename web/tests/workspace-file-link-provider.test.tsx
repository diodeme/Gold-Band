/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { resolveWorkspaceFileLink } from '@/api';
import { Markdown, useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import {
  ConversationWorkspaceStore,
  createDraftConversationWorkspaceScope,
  fileWorkspaceResourceKey,
  RightWorkspaceProvider,
  useRightWorkspace,
} from '@/components/workspace/right-workspace-context';
import { WorkspaceFileLinkProvider } from '@/components/workspace/files/WorkspaceFileLinkProvider';
import { fileContentStore } from '@/components/workspace/files/file-content-store';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, resolveWorkspaceFileLink: vi.fn() };
});

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.mocked(resolveWorkspaceFileLink).mockReset();
});

function FileLinkHarness() {
  const handler = useMarkdownResourceLinkHandler();
  const workspace = useRightWorkspace();
  const fileBrowser = workspace.tabs.find((tab) => tab.kind === 'file-browser');
  const file = fileBrowser?.selectedFile;
  return (
    <>
      <button type="button" onClick={() => handler?.openLocalFile('docs/README.md#L47')}>open line</button>
      <output data-line={file?.target?.line ?? ''} data-target-revision={file?.targetRevision ?? 0} />
    </>
  );
}

function FailingFileLinkHarness() {
  const workspace = useRightWorkspace();
  return (
    <>
      <Markdown>{'[roadmap.md](/D:/repo/roadmap.md:12)'}</Markdown>
      <output data-tab-count={workspace.tabs.length} />
    </>
  );
}

describe('workspace file link target lifecycle', () => {
  it('creates a new positioning intent when the same line link is clicked again', async () => {
    vi.mocked(resolveWorkspaceFileLink).mockResolvedValue({
      locator: {
        projectId: 'project-1',
        canonicalPath: 'D:\\repo\\docs\\README.md',
        relativePath: 'docs/README.md',
        scope: 'workspace',
      },
      target: { line: 47, column: null, endLine: null },
      externalAccessGrant: null,
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const scope = createDraftConversationWorkspaceScope('project-1');
    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={scope} store={new ConversationWorkspaceStore()}>
          <WorkspaceFileLinkProvider>
            <FileLinkHarness />
          </WorkspaceFileLinkProvider>
        </RightWorkspaceProvider>,
      ));
      const button = container.querySelector('button')!;

      await act(async () => button.click());
      expect(container.querySelector('output')?.dataset).toMatchObject({ line: '47', targetRevision: '1' });

      await act(async () => button.click());
      expect(container.querySelector('output')?.dataset).toMatchObject({ line: '47', targetRevision: '2' });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps resolver failures on the clicked link without creating a fake file resource', async () => {
    vi.mocked(resolveWorkspaceFileLink).mockRejectedValue({
      code: 'workspace-file.path-invalid',
      params: { path: '/D:/repo/roadmap.md:12' },
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const scope = createDraftConversationWorkspaceScope('project-1');
    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={scope} store={new ConversationWorkspaceStore()}>
          <WorkspaceFileLinkProvider>
            <FailingFileLinkHarness />
          </WorkspaceFileLinkProvider>
        </RightWorkspaceProvider>,
      ));

      await act(async () => container.querySelector<HTMLAnchorElement>('a')?.click());

      expect(resolveWorkspaceFileLink).toHaveBeenCalledWith(
        'project-1',
        '/D:/repo/roadmap.md:12',
        undefined,
      );
      expect(container.querySelector('output')?.dataset.tabCount).toBe('0');
      expect(container.querySelector('[role="alert"]')?.getAttribute('data-workspace-file-link-error'))
        .toBe('workspace-file.path-invalid');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('rotates the grant when the same external file link is opened again', async () => {
    const locator = {
      projectId: 'project-1',
      canonicalPath: 'D:\\outside\\roadmap.md',
      relativePath: null,
      scope: 'external' as const,
    };
    const firstGrant = {
      token: 'grant-1',
      permissions: ['read', 'write'] as Array<'read' | 'write'>,
      expiresAtMs: '9999999999998',
    };
    const secondGrant = { ...firstGrant, token: 'grant-2', expiresAtMs: '9999999999999' };
    vi.mocked(resolveWorkspaceFileLink)
      .mockResolvedValueOnce({ locator, target: null, externalAccessGrant: firstGrant })
      .mockResolvedValueOnce({ locator, target: null, externalAccessGrant: secondGrant });
    const primeExternalGrant = vi.spyOn(fileContentStore, 'primeExternalGrant').mockImplementation(() => undefined);
    const reauthorize = vi.spyOn(fileContentStore, 'reauthorize')
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const scope = createDraftConversationWorkspaceScope('project-1');
    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={scope} store={new ConversationWorkspaceStore()}>
          <WorkspaceFileLinkProvider>
            <FileLinkHarness />
          </WorkspaceFileLinkProvider>
        </RightWorkspaceProvider>,
      ));
      const button = container.querySelector('button')!;

      await act(async () => button.click());
      await act(async () => button.click());

      const key = fileWorkspaceResourceKey('project-1', locator.canonicalPath);
      expect(primeExternalGrant).toHaveBeenCalledOnce();
      expect(primeExternalGrant).toHaveBeenCalledWith(
        key,
        'project-1',
        locator.canonicalPath,
        firstGrant,
      );
      expect(reauthorize).toHaveBeenCalledTimes(2);
      expect(reauthorize).toHaveBeenNthCalledWith(1, key, firstGrant);
      expect(reauthorize).toHaveBeenNthCalledWith(2, key, secondGrant);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
