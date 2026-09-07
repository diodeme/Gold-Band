/** @vitest-environment jsdom */

import React, { act, forwardRef, useImperativeHandle } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';

const imageActionMocks = vi.hoisted(() => ({
  copy: vi.fn(() => Promise.resolve()),
  save: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('@/lib/image-actions', () => ({
  copyImageAsset: imageActionMocks.copy,
  IMAGE_ACTION_FEEDBACK_DURATION_MS: 1_800,
  saveImageAssetAs: imageActionMocks.save,
}));

vi.mock('react-zoom-pan-pinch', () => ({
  TransformWrapper: forwardRef(function TransformWrapper(
    props: { children: React.ReactNode },
    ref: React.ForwardedRef<unknown>,
  ) {
    useImperativeHandle(ref, () => ({
      zoomOut: vi.fn(), centerView: vi.fn(), resetTransform: vi.fn(), zoomIn: vi.fn(), setTransform: vi.fn(),
    }));
    return <div>{props.children}</div>;
  }),
  TransformComponent: (props: { children: React.ReactNode }) => <div>{props.children}</div>,
}));

vi.mock('@/components/ui/context-menu', () => ({
  ContextMenu: (props: { children: React.ReactNode }) => <div data-testid="image-context-menu">{props.children}</div>,
  ContextMenuTrigger: (props: { children: React.ReactNode }) => <>{props.children}</>,
  ContextMenuContent: (props: { children: React.ReactNode }) => <div>{props.children}</div>,
  ContextMenuItem: (props: { children: React.ReactNode; disabled?: boolean; onSelect?: () => void }) => (
    <button type="button" disabled={props.disabled} onClick={props.onSelect}>{props.children}</button>
  ),
}));

import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('workspace image context menu', () => {
  afterEach(() => {
    vi.useRealTimers();
    imageActionMocks.copy.mockClear();
    imageActionMocks.save.mockClear();
    document.body.replaceChildren();
  });

  it('offers copy and save-as for a draft image and reports action completion', async () => {
    vi.useFakeTimers();
    const attachment = {
      id: 'image', name: 'image.png', size: 12, mime: 'image/png',
      path: 'D:/image.png', previewUrl: 'asset://image', source: 'dialog' as const,
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <WorkspaceImageCanvas src="asset://image" alt="image.png" imageActionAsset={attachment} />,
      ));

      const buttons = Array.from(container.querySelectorAll('button'));
      const copyButton = buttons.find((button) => button.textContent?.includes('复制图片'));
      const saveButton = buttons.find((button) => button.textContent?.includes('图片另存为'));
      expect(copyButton).toBeTruthy();
      expect(saveButton).toBeTruthy();

      await act(async () => copyButton?.click());
      expect(imageActionMocks.copy).toHaveBeenCalledWith(attachment);
      expect(container.textContent).toContain('图片已复制');

      await act(async () => saveButton?.click());
      expect(imageActionMocks.save).toHaveBeenCalledWith(attachment);
      expect(container.textContent).toContain('图片已保存');

      await act(async () => vi.advanceTimersByTime(1_800));
      expect(container.textContent).not.toContain('图片已保存');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not expose attachment actions for a read-only image without an action source', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<WorkspaceImageCanvas src="asset://image" alt="image.png" />));
      expect(container.querySelector('[data-testid="image-context-menu"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
