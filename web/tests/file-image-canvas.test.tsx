/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { FileWorkspaceResource } from '@/components/workspace/right-workspace-context';

vi.mock('@/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/api')>(),
  readFileResource: vi.fn(),
  releaseWorkspaceFilePreview: vi.fn().mockResolvedValue(undefined),
  workspaceFilePreviewUrl: (token: string, paused: boolean) => `https://test.invalid/${token}?paused=${paused}`,
}));
vi.mock('@/components/workspace/right-workspace-context', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/components/workspace/right-workspace-context')>(),
  useRightWorkspaceCommands: () => ({ scopeKey: 'draft:p', openResource: vi.fn() }),
}));

import { readFileResource } from '@/api';
import { FileContent } from '@/components/workspace/files/FileWorkspacePanel';
import { fileContentStore } from '@/components/workspace/files/file-content-store';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const resource: FileWorkspaceResource = {
  kind: 'file', key: 'file:p:test.png', scopeKey: 'draft:p', projectId: 'p', workspacePath: null,
  title: 'test.png', attention: false, target: null, targetRevision: 0,
  locator: { projectId: 'p', canonicalPath: 'D:/test.png', relativePath: 'test.png', scope: 'workspace' },
};

describe('file image preview uses the shared canvas', () => {
  beforeEach(async () => {
    vi.useFakeTimers();
    vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
    vi.mocked(readFileResource).mockResolvedValue({
      kind: 'image', locator: resource.locator, name: 'test.png', width: 1365, height: 900,
      animated: true, sourceEditable: false, mimeType: 'image/gif',
      revision: { contentHash: 'image', byteLength: 100, modifiedAtNs: '1' },
      previewGrant: { token: 'image', expiresAtMs: String(Date.now() + 3600000) },
      externalAccessGrant: null,
    } as Awaited<ReturnType<typeof readFileResource>>);
    await fileContentStore.load(resource);
  });
  afterEach(async () => {
    await fileContentStore.release(resource.key);
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
    document.body.replaceChildren();
  });

  it.each([false, true])('zooms from the file entry and retains GIF view state (ctrlKey=%s)', async (ctrlKey) => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const render = () => root.render(<TooltipProvider><FileContent resource={resource} /></TooltipProvider>);
    try {
      await act(async () => render());
      const image = container.querySelector('img')!;
      image.dispatchEvent(new WheelEvent('wheel', {
        bubbles: true, cancelable: true, deltaY: -100, clientX: 100, clientY: 80, ctrlKey,
      }));
      await act(async () => vi.advanceTimersToNextFrame());
      expect(container.textContent).toContain('135%');
      const transform = container.querySelector<HTMLElement>('.react-transform-component')!;
      const zoomed = transform.style.transform;
      expect(zoomed).toContain('scale(1.349858');
      expect(container.textContent).toContain('1365 × 900');

      // Changing the GIF URL must not remount/reset the transform.
      await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="暂停动画"]')!.click());
      expect(container.querySelector('img')).toBe(image);
      expect(image.src).toContain('paused=true');
      expect(transform.style.transform).toBe(zoomed);

      // Leaving and returning to the file restores its scale and translation.
      await act(async () => root.render(null));
      await act(async () => render());
      const restored = container.querySelector<HTMLElement>('.react-transform-component')!.style.transform;
      const values = (transform: string) => transform.match(/-?\d+(?:\.\d+)?/g)!.map(Number);
      values(restored).forEach((value, index) => expect(value).toBeCloseTo(values(zoomed)[index], 2));
      expect(container.textContent).toContain('135%');
      expect(vi.mocked(readFileResource)).toHaveBeenCalledTimes(1);
      const snapshot = await vi.mocked(readFileResource).mock.results[0].value;
      vi.mocked(readFileResource).mockResolvedValue({ ...snapshot, previewGrant: { ...snapshot.previewGrant, token: 'renewed' } });
      const restoredImage = container.querySelector('img');
      await act(async () => fileContentStore.load(resource, false, true, true));
      expect(container.querySelector('img')).toBe(restoredImage);
      expect(restoredImage?.src).toContain('/renewed?');
      expect(container.querySelector<HTMLElement>('.react-transform-component')!.style.transform).toBe(restored);

      const viewport = container.querySelector('[data-workspace-image-viewport]')!;
      Object.defineProperties(viewport, { clientWidth: { value: 600 }, clientHeight: { value: 800 } });
      await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="适应窗口"]')!.click());
      await act(async () => vi.advanceTimersByTime(300));
      expect(container.textContent).toContain('41%');
      expect(container.querySelector('button[aria-label="重置图片"]')).toBeNull();
      await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="适应窗口"]')!.click());
      await act(async () => vi.advanceTimersByTime(300));
      expect(fileContentStore.imageViewState(resource.key)?.scale).toBeCloseTo(560 / 1365);
      await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="原始大小"]')!.click());
      await act(async () => vi.advanceTimersByTime(300));
      expect(fileContentStore.imageViewState(resource.key)?.scale).toBe(1);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
