/** @vitest-environment jsdom */

import React, { act } from 'react';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { WorkspaceFileEditor } from '@/components/workspace/files/WorkspaceFileEditor';
import { WorkspaceFileLinkProvider } from '@/components/workspace/files/WorkspaceFileLinkProvider';
import { useMarkdownResourceLinkHandler } from '@/components/prompt-kit/markdown';
import {
  ConversationWorkspaceStore,
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
} from '@/components/workspace/right-workspace-context';
import { openWebTarget } from '@/components/workspace/browser/open-web-target';

vi.mock('@/components/workspace/browser/open-web-target', async () => {
  const actual = await vi.importActual<typeof import('@/components/workspace/browser/open-web-target')>(
    '@/components/workspace/browser/open-web-target',
  );
  return { ...actual, openWebTarget: vi.fn(async () => ({ status: 'opened', kind: 'browser', pageId: 'page-1' })) };
});

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  vi.mocked(openWebTarget).mockClear();
});

function OpenHtmlFromChat() {
  const handler = useMarkdownResourceLinkHandler();
  return <button type="button" onClick={() => void handler?.openLocalFile('docs/index.html')}>open html</button>;
}

describe('workspace HTML source entry', () => {
  it('does not intercept directory-tree HTML clicks into the built-in browser', () => {
    const source = readFileSync(resolve(process.cwd(), 'web/src/components/workspace/files/FileWorkspacePanel.tsx'), 'utf8');
    expect(source).not.toContain("classifyWebTarget(entry.canonicalPath) === 'local-html'");
    expect(source).not.toContain('openWebTarget(entry.canonicalPath');
    expect(source).toContain('isHtmlDocumentPath');
    expect(source).toContain('onOpenInBrowser');
    expect(source).toMatch(/fileContentStore\.flush\(resource\.key\)[\s\S]*openWebTarget/);
  });

  it('gives run-directory HTML the same source overlay without binding the tree to workspace commands', () => {
    const source = readFileSync(resolve(process.cwd(), 'web/src/components/workspace/ConversationDirectoryWorkspacePanel.tsx'), 'utf8');
    expect(source).toContain('function ConversationDirectoryTextPreview');
    expect(source).toContain('onOpenInBrowser={htmlDocument ? openHtmlInBrowser : undefined}');
    expect(source).not.toContain("classifyWebTarget");
    const panelBody = source.slice(source.indexOf('export function ConversationDirectoryWorkspacePanel'));
    expect(panelBody).not.toContain('useRightWorkspaceCommands()');
  });

  it('keeps conversation HTML references on the built-in browser path', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const scope = createDraftConversationWorkspaceScope('project-1');
    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={scope} store={new ConversationWorkspaceStore()}>
          <WorkspaceFileLinkProvider browserPreferences={{
            schemaVersion: 1,
            searchEngine: 'baidu',
            openLocalLinksInBrowser: true,
            openWebLinksInBrowser: true,
          }}
          >
            <OpenHtmlFromChat />
          </WorkspaceFileLinkProvider>
        </RightWorkspaceProvider>,
      ));
      await act(async () => container.querySelector('button')?.click());
      expect(openWebTarget).toHaveBeenCalledWith('docs/index.html', expect.objectContaining({
        projectId: 'project-1',
        scopeKey: scope.key,
      }));
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows a floating open-in-browser action on HTML source and keeps ordinary files without it', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onOpenInBrowser = vi.fn();
    const props = {
      documentKey: 'html-source',
      value: '<p>hi</p>',
      language: 'html',
      highlight: false,
      contentRevision: 1,
      target: null,
      targetRevision: 0,
      onChange: () => undefined,
      onSave: () => undefined,
      initialStateJson: null,
      onPersistState: () => undefined,
    } as const;
    try {
      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} editable onOpenInBrowser={onOpenInBrowser} />
        </TooltipProvider>,
      ));
      const button = container.querySelector<HTMLButtonElement>('[data-html-open-in-browser="true"]');
      expect(button).not.toBeNull();
      await act(async () => button?.click());
      expect(onOpenInBrowser).toHaveBeenCalledTimes(1);

      await act(async () => root.render(
        <TooltipProvider>
          <WorkspaceFileEditor {...props} documentKey="plain-source" language="text" editable />
        </TooltipProvider>,
      ));
      expect(container.querySelector('[data-html-open-in-browser="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
