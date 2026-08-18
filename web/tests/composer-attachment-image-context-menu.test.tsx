/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';

import type { AttachmentItem } from '@/lib/attachment-service';
import { TooltipProvider } from '@/components/ui/tooltip';

const imageActionMocks = vi.hoisted(() => ({
  copy: vi.fn(() => Promise.resolve()),
  save: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('@/lib/image-actions', () => ({
  copyAttachmentImage: imageActionMocks.copy,
  saveAttachmentImageAs: imageActionMocks.save,
}));

import { ComposerContextArea } from '@/components/shared/ComposerContextArea';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class PassiveResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const imageAttachment: AttachmentItem = {
  id: 'image',
  name: 'shot.png',
  size: 4,
  mime: 'image/png',
  file: new File([new Uint8Array([1, 2, 3, 4])], 'shot.png', { type: 'image/png' }),
  previewUrl: 'blob:shot',
  source: 'paste',
};

async function openContextMenu(target: Element) {
  await act(async () => {
    target.dispatchEvent(new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      button: 2,
      buttons: 2,
      clientX: 12,
      clientY: 12,
    }));
  });
}

describe('composer image attachment context menu', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', PassiveResizeObserver);
    imageActionMocks.copy.mockClear();
    imageActionMocks.save.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it('offers the shared image actions from an image thumbnail', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ComposerContextArea
              attachments={[imageAttachment]}
              onRemoveAttachment={vi.fn()}
              onPreviewAttachment={vi.fn()}
            />
          </TooltipProvider>,
        );
      });

      const thumbnail = container.querySelector('img');
      expect(thumbnail).not.toBeNull();
      await openContextMenu(thumbnail!);

      const menu = document.querySelector('[data-slot="context-menu-content"]');
      expect(menu?.textContent).toContain('复制图片');
      expect(menu?.textContent).toContain('图片另存为');

      const copyItem = Array.from(document.querySelectorAll<HTMLElement>('[data-slot="context-menu-item"]'))
        .find((item) => item.textContent?.includes('复制图片'));
      await act(async () => copyItem?.click());

      expect(imageActionMocks.copy).toHaveBeenCalledOnce();
      expect(imageActionMocks.copy).toHaveBeenCalledWith(imageAttachment);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not attach image actions to non-image attachments', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ComposerContextArea
              attachments={[{
                id: 'text',
                name: 'notes.txt',
                size: 4,
                mime: 'text/plain',
                source: 'dialog',
              }]}
              onRemoveAttachment={vi.fn()}
              onPreviewAttachment={vi.fn()}
            />
          </TooltipProvider>,
        );
      });

      const previewButton = container.querySelector('button[aria-label^="notes.txt"]');
      expect(previewButton).not.toBeNull();
      await openContextMenu(previewButton!);

      expect(document.querySelector('[data-slot="context-menu-content"]')).toBeNull();
      expect(imageActionMocks.copy).not.toHaveBeenCalled();
      expect(imageActionMocks.save).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
