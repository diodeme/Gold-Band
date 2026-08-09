/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { resolveWorkspaceFileLink } from '@/api';
import { useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import {
  ConversationWorkspaceStore,
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspace,
} from '@/components/workspace/right-workspace-context';
import { WorkspaceFileLinkProvider } from '@/components/workspace/files/WorkspaceFileLinkProvider';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, resolveWorkspaceFileLink: vi.fn() };
});

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
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
});
