/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';

const imageActionMocks = vi.hoisted(() => ({
  copy: vi.fn(() => Promise.resolve()),
  save: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('@/lib/image-actions', () => ({
  copyAttachmentImage: imageActionMocks.copy,
  saveAttachmentImageAs: imageActionMocks.save,
}));

import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class PassiveResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe('workspace image context menu DOM interaction', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', PassiveResizeObserver);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it('leaves right click to Radix while retaining left-button image panning', async () => {
    const attachment = {
      id: 'image', name: 'image.png', size: 12, mime: 'image/png',
      path: 'D:/image.png', previewUrl: 'asset://image', source: 'dialog' as const,
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <WorkspaceImageCanvas src="asset://image" alt="image.png" attachment={attachment} />,
        );
      });

      const image = container.querySelector('img');
      expect(image).not.toBeNull();
      const contextMenuTrigger = image?.closest('[data-slot="context-menu-trigger"]');
      expect(contextMenuTrigger?.tagName).toBe('SPAN');
      expect(contextMenuTrigger).not.toBe(image);

      const leftMouseDown = new MouseEvent('mousedown', {
        bubbles: true,
        cancelable: true,
        button: 0,
        buttons: 1,
        clientX: 24,
        clientY: 32,
      });
      image?.dispatchEvent(leftMouseDown);
      expect(leftMouseDown.defaultPrevented).toBe(true);
      window.dispatchEvent(new MouseEvent('mouseup', {
        bubbles: true,
        button: 0,
        buttons: 0,
        clientX: 24,
        clientY: 32,
      }));

      const rightMouseDown = new MouseEvent('mousedown', {
        bubbles: true,
        cancelable: true,
        button: 2,
        buttons: 2,
        clientX: 24,
        clientY: 32,
      });
      image?.dispatchEvent(rightMouseDown);
      expect(rightMouseDown.defaultPrevented).toBe(false);

      await act(async () => {
        image?.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          button: 2,
          buttons: 2,
          clientX: 24,
          clientY: 32,
        }));
      });

      const menu = document.querySelector('[data-slot="context-menu-content"]');
      expect(menu).not.toBeNull();
      expect(menu?.textContent).toContain('复制图片');
      expect(menu?.textContent).toContain('图片另存为');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
