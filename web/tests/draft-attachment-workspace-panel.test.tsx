/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DraftAttachmentWorkspaceResource } from '@/components/workspace/right-workspace-context';
import '@/i18n';

vi.mock('@/components/workspace/files/ReadonlyTextWorkspaceViewer', () => ({
  ReadonlyTextWorkspaceViewer: (props: { documentKey: string; name: string; value: string }) => (
    <div
      data-testid="readonly-text-workspace"
      data-document-key={props.documentKey}
      data-name={props.name}
    >
      {props.value}
    </div>
  ),
}));

vi.mock('@/components/workspace/files/WorkspaceImageCanvas', () => ({
  WorkspaceImageCanvas: (props: { src: string; alt: string; attachment?: { id: string } }) => (
    <div data-testid="workspace-image" data-src={props.src} data-attachment-id={props.attachment?.id}>{props.alt}</div>
  ),
}));

import { DraftAttachmentWorkspacePanel } from '@/components/workspace/files/DraftAttachmentWorkspacePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function resource(
  attachment: DraftAttachmentWorkspaceResource['attachment'],
): DraftAttachmentWorkspaceResource {
  return {
    kind: 'draft-attachment',
    key: `draft-attachment:draft:project-1:${attachment.id}`,
    scopeKey: 'draft:project-1',
    projectId: 'project-1',
    title: attachment.name,
    attention: false,
    attachment,
  };
}

describe('draft attachment workspace panel', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it('loads text lazily from its revision-bound URL and opens Markdown in the shared viewer', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('# Attachment'));
    vi.stubGlobal('fetch', fetchMock);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<DraftAttachmentWorkspacePanel resource={resource({
          id: 'notes',
          name: 'notes.md',
          size: 12,
          mime: 'text/markdown',
          path: 'D:/notes.md',
          contentUrl: 'asset://notes',
          source: 'dialog',
        })} />);
      });

      expect(fetchMock).toHaveBeenCalledWith('asset://notes', { cache: 'no-store' });
      const viewer = container.querySelector<HTMLElement>('[data-testid="readonly-text-workspace"]');
      expect(viewer?.dataset.name).toBe('notes.md');
      expect(viewer?.textContent).toBe('# Attachment');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps image attachments on the existing workspace canvas without reading text', async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<DraftAttachmentWorkspacePanel resource={resource({
          id: 'image',
          name: 'image.png',
          size: 12,
          mime: 'image/png',
          previewUrl: 'asset://image',
          source: 'dialog',
        })} />);
      });

      expect(container.querySelector<HTMLElement>('[data-testid="workspace-image"]')?.dataset.src).toBe('asset://image');
      expect(container.querySelector<HTMLElement>('[data-testid="workspace-image"]')?.dataset.attachmentId).toBe('image');
      expect(container.querySelector('[data-testid="readonly-text-workspace"]')).toBeNull();
      expect(fetchMock).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
