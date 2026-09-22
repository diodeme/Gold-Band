/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';

import {
  ConversationComposerDraftBoundary,
  type ConversationComposerDraftBoundaryHandle,
} from '@/components/conversation/ConversationComposerDraftBoundary';
import { useConversationComposerDraft } from '@/lib/conversation-composer-draft';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('ConversationComposerDraftBoundary', () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  it('lets the application boundary switch the draft workspace without dropping file references', () => {
    container = document.body.appendChild(document.createElement('div'));
    root = createRoot(container);
    const boundaryRef = { current: null as ConversationComposerDraftBoundaryHandle | null };
    const observedFiles: unknown[] = [];
    let addProjectAFile: (() => void) | null = null;

    function Consumer() {
      const draft = useConversationComposerDraft();
      observedFiles.length = 0;
      observedFiles.push(...draft.draft.workspaceFiles);
      addProjectAFile = () => draft.setWorkspaceFiles([{
        id: 'project-a-file',
        projectId: 'project-a',
        relativePath: 'src/a.ts',
        name: 'a.ts',
        byteLength: 1,
        mimeType: 'text/plain',
      }]);
      return null;
    }

    act(() => {
      root?.render(
        <ConversationComposerDraftBoundary ref={boundaryRef}>
          <Consumer />
        </ConversationComposerDraftBoundary>,
      );
    });
    act(() => {
      boundaryRef.current?.changeWorkspace('project-a');
    });
    act(() => {
      addProjectAFile?.();
    });
    act(() => {
      boundaryRef.current?.changeWorkspace('project-b');
    });

    expect(observedFiles).toEqual([
      expect.objectContaining({ projectId: 'project-a', relativePath: 'src/a.ts' }),
    ]);
  });
});
