/** @vitest-environment jsdom */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  browserCreatePage: vi.fn(async (input: { pageId: string; url: string }) => ({
    pageId: input.pageId,
    url: input.url,
    label: `gb-${input.pageId}`,
  })),
  browserSetBounds: vi.fn(async () => undefined),
  browserShowPage: vi.fn(async () => undefined),
  browserHideAll: vi.fn(async () => undefined),
  browserNavigate: vi.fn(async (input: { pageId: string; url: string }) => ({
    pageId: input.pageId,
    url: input.url,
    label: `gb-${input.pageId}`,
  })),
  browserClosePage: vi.fn(async () => undefined),
  browserSetViewMode: vi.fn(async () => undefined),
  browserDiscardAll: vi.fn(async () => undefined),
  browserStop: vi.fn(async () => undefined),
  subscribeBrowserPageEvents: vi.fn(async () => () => undefined),
}));

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => api,
}));

import { browserSessionStore } from '@/components/workspace/browser/browser-session-store';
import { browserWebviewHost } from '@/components/workspace/browser/browser-webview-host';
import { overlayOwnerAttribute, rightWorkspaceOverlayOwner } from '@/lib/portal-container';

const bounds = { x: 20, y: 40, width: 640, height: 480 };

function livePage(url = 'about:blank') {
  const pageId = url === 'about:blank'
    ? browserSessionStore.addBlankPage()
    : browserSessionStore.openUrl(url);
  return browserSessionStore.page(pageId)!;
}

describe('browser webview host lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.browserCreatePage.mockImplementation(async (input) => ({
      pageId: input.pageId,
      url: input.url,
      label: `gb-${input.pageId}`,
    }));
    api.browserNavigate.mockImplementation(async (input) => ({
      pageId: input.pageId,
      url: input.url,
      label: `gb-${input.pageId}`,
    }));
    api.browserHideAll.mockResolvedValue(undefined);
    api.browserShowPage.mockResolvedValue(undefined);
    api.browserDiscardAll.mockResolvedValue(undefined);
  });

  afterEach(() => {
    browserWebviewHost.resetForTests();
    browserSessionStore.resetForTests();
    document.body.replaceChildren();
  });

  it('opens an internal page when the native webview reports a new-window url', () => {
    const page = livePage('https://www.google.com/search?q=github');
    browserWebviewHost.handleNativeEvent({
      kind: 'new-window',
      pageId: page.pageId,
      url: 'https://github.com/',
    });
    expect(browserSessionStore.snapshot().pages).toHaveLength(2);
    expect(browserSessionStore.page(browserSessionStore.snapshot().activePageId!)?.url).toBe('https://github.com/');
  });

  it('does not treat a closed menu as a blocking overlay', () => {
    document.body.innerHTML = '<div data-slot="dropdown-menu-content" data-state="closed"></div>';
    expect(browserWebviewHost.hasBlockingOverlay()).toBe(false);
    document.body.innerHTML = '<div data-slot="dropdown-menu-content" data-state="open"></div>';
    expect(browserWebviewHost.hasBlockingOverlay()).toBe(true);
  });

  it('does not treat the compact right-workspace sheet overlay as blocking', () => {
    document.body.innerHTML = `
      <div id="gold-band-overlay-portal-host">
        <div data-slot="sheet-overlay" data-state="open" ${overlayOwnerAttribute}="${rightWorkspaceOverlayOwner}"></div>
        <div data-slot="sheet-content" data-state="open">
          <div data-right-workspace-presentation="sheet"></div>
        </div>
      </div>
    `;
    expect(browserWebviewHost.hasBlockingOverlay()).toBe(false);

    document.querySelector('[data-slot="sheet-overlay"]')?.removeAttribute(overlayOwnerAttribute);
    expect(browserWebviewHost.hasBlockingOverlay()).toBe(true);
  });

  it('does not create a native page for the portal blank url', async () => {
    const page = livePage();
    await browserWebviewHost.ensurePage(page, bounds, true);
    expect(api.browserCreatePage).not.toHaveBeenCalled();
    expect(api.browserShowPage).not.toHaveBeenCalled();
  });

  it('keeps placeholder bounds so a portal page can create at the measured size', async () => {
    const page = livePage();
    await browserWebviewHost.ensurePage(page, bounds, true);
    await browserWebviewHost.commitNavigation(page.pageId, 'https://baidu.com');
    expect(api.browserCreatePage).toHaveBeenCalledWith(expect.objectContaining({
      pageId: page.pageId,
      url: 'https://baidu.com/',
      bounds,
    }));
    expect(api.browserNavigate).not.toHaveBeenCalled();
  });

  it('commits the address and creates the native page when it is not live yet', async () => {
    const page = livePage();
    await browserWebviewHost.commitNavigation(page.pageId, 'https://baidu.com');
    expect(browserSessionStore.page(page.pageId)?.url).toBe('https://baidu.com/');
    expect(browserSessionStore.page(page.pageId)?.loading).toBe(true);
    expect(api.browserCreatePage).toHaveBeenCalledWith(expect.objectContaining({
      pageId: page.pageId,
      url: 'https://baidu.com/',
    }));
    expect(api.browserNavigate).not.toHaveBeenCalled();
  });

  it('applies the latest placeholder bounds after a slow native create', async () => {
    let releaseCreate: ((value: { pageId: string; url: string; label: string }) => void) | null = null;
    api.browserCreatePage.mockImplementationOnce((input) => new Promise((resolve) => {
      releaseCreate = () => resolve({
        pageId: input.pageId,
        url: input.url,
        label: `gb-${input.pageId}`,
      });
    }));
    const page = livePage('https://example.com');
    const first = { x: 20, y: 40, width: 400, height: 400 };
    const settled = { x: 20, y: 40, width: 640, height: 720 };
    const creating = browserWebviewHost.ensurePage(page, first, true);
    await vi.waitFor(() => expect(releaseCreate).not.toBeNull());
    const updating = browserWebviewHost.ensurePage(page, settled, true);
    expect(api.browserCreatePage).toHaveBeenCalledTimes(1);
    expect(api.browserCreatePage).toHaveBeenCalledWith(expect.objectContaining({ bounds: first }));
    releaseCreate!();
    await creating;
    await updating;
    expect(api.browserSetBounds).toHaveBeenCalledWith({
      pageId: page.pageId,
      bounds: settled,
    });
  });

  it('flushes the latest bounds when multiple resizes land in the same frame', async () => {
    const callbacks: FrameRequestCallback[] = [];
    const originalRaf = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      callbacks.push(callback);
      return callbacks.length;
    }) as typeof requestAnimationFrame;
    try {
      const page = livePage('https://example.com');
      browserSessionStore.markLive(page.pageId, true);
      await browserWebviewHost.ensurePage(page, bounds, true);
      api.browserSetBounds.mockClear();
      browserWebviewHost.scheduleBounds(page.pageId, { ...bounds, width: 500 });
      browserWebviewHost.scheduleBounds(page.pageId, { ...bounds, width: 700 });
      expect(callbacks).toHaveLength(1);
      callbacks.splice(0).forEach((callback) => callback(0));
      expect(api.browserSetBounds).toHaveBeenCalledTimes(1);
      expect(api.browserSetBounds).toHaveBeenCalledWith({
        pageId: page.pageId,
        bounds: { ...bounds, width: 700 },
      });
    } finally {
      globalThis.requestAnimationFrame = originalRaf;
    }
  });

  it('navigates after a pending create instead of racing a missing webview', async () => {
    let releaseCreate: ((value: { pageId: string; url: string; label: string }) => void) | null = null;
    api.browserCreatePage.mockImplementationOnce(() => new Promise((resolve) => {
      releaseCreate = resolve;
    }));
    const page = livePage('https://example.com');
    const ensuring = browserWebviewHost.ensurePage(page, bounds, true);
    const committing = browserWebviewHost.commitNavigation(page.pageId, 'https://baidu.com');
    await vi.waitFor(() => expect(releaseCreate).not.toBeNull());
    releaseCreate!({ pageId: page.pageId, url: 'https://example.com/', label: 'gb-pending' });
    await ensuring;
    await committing;
    expect(api.browserNavigate).toHaveBeenCalledWith({
      pageId: page.pageId,
      url: 'https://baidu.com/',
    });
  });

  it('coalesces repeated navigation to the same target while the native command is pending', async () => {
    let releaseNavigate: ((value: { pageId: string; url: string; label: string }) => void) | null = null;
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    api.browserNavigate.mockImplementationOnce((input) => new Promise((resolve) => {
      releaseNavigate = () => resolve({
        pageId: input.pageId,
        url: input.url,
        label: `gb-${input.pageId}`,
      });
    }));

    const first = browserWebviewHost.commitNavigation(page.pageId, 'file:///E:/demo/index.html');
    const repeated = browserWebviewHost.commitNavigation(page.pageId, 'file:///E:/demo/index.html');
    await vi.waitFor(() => expect(releaseNavigate).not.toBeNull());

    expect(api.browserNavigate).toHaveBeenCalledTimes(1);
    releaseNavigate!();
    await Promise.all([first, repeated]);
  });

  it('keeps the last confirmed url when native navigation fails', async () => {
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    api.browserNavigate.mockRejectedValueOnce({
      code: 'browser.local_html.grant_failed',
      params: {},
    });

    await expect(
      browserWebviewHost.commitNavigation(page.pageId, 'file:///E:/demo/index.html'),
    ).rejects.toMatchObject({ code: 'browser.local_html.grant_failed' });

    expect(browserSessionStore.page(page.pageId)).toMatchObject({
      url: 'https://example.com/',
      loading: false,
    });
    expect(browserSessionStore.snapshot().noticeCode).toBe('browser.local_html.grant_failed');
  });

  it('suppresses show while hide is in flight so a leftover webview cannot cover the empty workspace', async () => {
    let releaseHide: (() => void) | null = null;
    api.browserHideAll.mockImplementationOnce(() => new Promise((resolve) => {
      releaseHide = resolve;
    }));
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    await browserWebviewHost.ensurePage(page, bounds, true);
    const hiding = browserWebviewHost.hideAll();
    const suppressing = browserWebviewHost.suppress();
    const ensuring = browserWebviewHost.ensurePage(page, bounds, true);
    releaseHide?.();
    await Promise.all([hiding, suppressing, ensuring]);
    const showsAfterSuppress = api.browserShowPage.mock.calls.length;
    await browserWebviewHost.ensurePage(page, bounds, true);
    expect(api.browserShowPage.mock.calls.length).toBe(showsAfterSuppress);
    expect(api.browserHideAll).toHaveBeenCalled();
  });

  it('notifies the viewport when an overlay closes so the current page can be shown again', () => {
    const listener = vi.fn();
    browserWebviewHost.onOverlayChange(listener);
    browserWebviewHost.setOverlayOpen(true);
    browserWebviewHost.setOverlayOpen(false);
    expect(listener).toHaveBeenCalled();
  });

  it('notifies the viewport and allows show after resume so returning to the browser is not a white pane', async () => {
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    await browserWebviewHost.ensurePage(page, bounds, true);
    const listener = vi.fn();
    browserWebviewHost.onVisibilityChange(listener);
    browserWebviewHost.suppress();
    api.browserShowPage.mockClear();
    await browserWebviewHost.ensurePage(page, bounds, true);
    expect(api.browserShowPage).not.toHaveBeenCalled();
    browserWebviewHost.resume();
    expect(listener).toHaveBeenCalled();
    await browserWebviewHost.ensurePage(page, bounds, true);
    expect(api.browserShowPage).toHaveBeenCalledWith({ pageId: page.pageId });
  });

  it('keeps the live webview when switching view mode so back and forward history survive', async () => {
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    await browserWebviewHost.ensurePage(page, bounds, true);
    api.browserCreatePage.mockClear();
    api.browserClosePage.mockClear();
    await browserWebviewHost.setViewMode(page.pageId, 'mobile');
    expect(browserSessionStore.page(page.pageId)?.viewMode).toBe('mobile');
    expect(browserSessionStore.page(page.pageId)?.live).toBe(true);
    expect(api.browserClosePage).not.toHaveBeenCalled();
    expect(api.browserCreatePage).not.toHaveBeenCalled();
    expect(api.browserSetViewMode).toHaveBeenCalledWith({
      pageId: page.pageId,
      viewMode: 'mobile',
    });
  });

  it('clears loading when native create fails so the loader cannot cover the workspace forever', async () => {
    const page = livePage('https://example.com');
    expect(browserSessionStore.page(page.pageId)?.loading).toBe(true);
    api.browserCreatePage.mockRejectedValueOnce(new Error('native create failed'));

    await expect(browserWebviewHost.ensurePage(page, bounds, true)).rejects.toThrow('native create failed');
    expect(browserSessionStore.page(page.pageId)?.live).toBe(false);
    expect(browserSessionStore.page(page.pageId)?.loading).toBe(false);
  });

  it('applies native eviction even when the replacement webview fails to create', async () => {
    const retained = livePage('https://retained.example');
    const evicted = livePage('https://evicted.example');
    const created = livePage('https://created.example');
    browserSessionStore.markLive(retained.pageId, true);
    browserSessionStore.markLive(evicted.pageId, true);
    api.browserCreatePage.mockImplementationOnce(async () => {
      browserWebviewHost.handleNativeEvent({ kind: 'discarded', pageId: evicted.pageId });
      throw new Error('replacement create failed');
    });

    await expect(browserWebviewHost.ensurePage(created, bounds, true)).rejects.toThrow('replacement create failed');

    expect(browserSessionStore.page(retained.pageId)?.live).toBe(true);
    expect(browserSessionStore.page(evicted.pageId)?.live).toBe(false);
    expect(browserSessionStore.page(created.pageId)?.live).toBe(false);
    expect(api.browserClosePage).not.toHaveBeenCalled();
  });

  it('keeps pages live when native discard fails so cleanup remains retryable', async () => {
    const page = livePage('https://example.com');
    browserSessionStore.markLive(page.pageId, true);
    api.browserDiscardAll.mockRejectedValueOnce(new Error('native close failed'));

    await expect(browserWebviewHost.discardAll()).rejects.toThrow('native close failed');
    expect(browserSessionStore.page(page.pageId)?.live).toBe(true);

    await browserWebviewHost.discardAll();
    expect(browserSessionStore.page(page.pageId)?.live).toBe(false);
    expect(api.browserDiscardAll).toHaveBeenCalledTimes(2);
  });
});
