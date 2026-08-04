/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const { streamdownRender } = vi.hoisted(() => ({ streamdownRender: vi.fn() }));

vi.mock('streamdown', () => ({
  defaultUrlTransform: (url: string) => url,
  Streamdown: ({ children }: { children: React.ReactNode }) => {
    streamdownRender();
    return <div>{children}</div>;
  },
}));

vi.mock('@/api', () => ({
  openExternalUrl: vi.fn(),
  resolveWorkspaceFileLink: vi.fn(),
}));

import { Markdown, useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import { WorkspaceFileLinkProvider } from '@/components/workspace/files/WorkspaceFileLinkProvider';
import {
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspaceCommands,
  type FileWorkspaceResource,
  type RightWorkspaceCommands,
} from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const historicalMessages = Array.from({ length: 8 }, (_, index) => `历史消息 ${index}：[文件](docs/file-${index}.ts)`);

function fileResource(index: number): FileWorkspaceResource {
  return {
    kind: 'file',
    key: `file:project-1:D:/repo/file-${index}.ts`,
    scopeKey: 'draft:project-1',
    projectId: 'project-1',
    title: `file-${index}.ts`,
    description: `file-${index}.ts`,
    attention: false,
    locator: {
      projectId: 'project-1',
      canonicalPath: `D:/repo/file-${index}.ts`,
      relativePath: `file-${index}.ts`,
      scope: 'workspace',
    },
    target: null,
    targetRevision: 1,
  };
}

afterEach(() => {
  document.body.replaceChildren();
  streamdownRender.mockClear();
});

describe('historical Markdown workspace isolation', () => {
  it('does not rerender completed messages while opening 15 files', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    let commands: RightWorkspaceCommands | null = null;
    let linkHandlerRenders = 0;

    function CommandCapture() {
      commands = useRightWorkspaceCommands();
      return null;
    }
    function HistoricalMessages() {
      useMarkdownResourceLinkHandler();
      linkHandlerRenders += 1;
      return <>{historicalMessages.map((message) => <Markdown key={message}>{message}</Markdown>)}</>;
    }

    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={createDraftConversationWorkspaceScope('project-1')}>
          <WorkspaceFileLinkProvider>
            <CommandCapture />
            <HistoricalMessages />
          </WorkspaceFileLinkProvider>
        </RightWorkspaceProvider>,
      ));
      const initialMarkdownRenders = streamdownRender.mock.calls.length;
      const initialLinkHandlerRenders = linkHandlerRenders;

      for (let index = 0; index < 15; index += 1) {
        await act(async () => { await commands!.openResource(fileResource(index)); });
      }

      expect(initialMarkdownRenders).toBe(historicalMessages.length);
      expect(streamdownRender).toHaveBeenCalledTimes(initialMarkdownRenders);
      expect(linkHandlerRenders).toBe(initialLinkHandlerRenders);
      expect(commands!.getResource(fileResource(14).key)).toMatchObject({ title: 'file-14.ts' });
    } finally {
      await act(async () => root.unmount());
    }
  });
});
