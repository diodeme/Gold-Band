import { getRuntimeApi } from '@/api/client';
import { overlayPortalHostId } from '@/lib/portal-container';
import { browserSessionStore, type BrowserPage, type BrowserViewMode, BLANK_BROWSER_URL, isBrowserPortalUrl } from './browser-session-store';

export interface BrowserBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BrowserNativePageEvent {
  kind: string;
  pageId: string;
  url?: string | null;
  title?: string | null;
}

const OVERLAY_SLOT_SELECTORS = [
  '[data-slot="dialog-overlay"]',
  '[data-slot="alert-dialog-overlay"]',
  '[data-slot="dropdown-menu-content"]',
  '[data-slot="context-menu-content"]',
  '[data-slot="popover-content"]',
].join(',');

function roundBounds(bounds: BrowserBounds): BrowserBounds {
  return {
    x: Math.round(bounds.x),
    y: Math.round(bounds.y),
    width: Math.max(1, Math.round(bounds.width)),
    height: Math.max(1, Math.round(bounds.height)),
  };
}

function boundsEqual(left: BrowserBounds | null, right: BrowserBounds | null) {
  if (!left || !right) return left === right;
  return left.x === right.x && left.y === right.y && left.width === right.width && left.height === right.height;
}

function api() {
  return getRuntimeApi();
}

class BrowserWebviewHost {
  private started = false;
  private startPromise: Promise<void> | null = null;
  private unlisten: (() => void) | null = null;
  private frame: number | null = null;
  private lastBounds = new Map<string, BrowserBounds>();
  private visiblePageId: string | null = null;
  private overlayOpen = false;
  private overlayObserver: MutationObserver | null = null;
  private visibilityListener: (() => void) | null = null;
  private readonly pendingCreates = new Map<string, Promise<void>>();
  private hidePromise: Promise<void> | null = null;
  private visibilityRevision = 0;
  private suppressed = false;

  async ensureStarted() {
    if (this.started) return;
    if (!this.startPromise) {
      this.startPromise = (async () => {
        this.unlisten = await api().subscribeBrowserPageEvents?.((event) => {
          this.handleNativeEvent(event);
        }) ?? null;
        this.watchOverlays();
        this.started = true;
      })();
    }
    await this.startPromise;
  }

  handleNativeEvent(event: BrowserNativePageEvent) {
    if (event.kind === 'new-window' && event.url) {
      browserSessionStore.openUrl(event.url);
      return;
    }
    browserSessionStore.applyNativeEvent(event);
  }

  async ensurePage(page: BrowserPage, bounds: BrowserBounds, visible: boolean) {
    await this.ensureStarted();
    const next = roundBounds(bounds);
    const previous = this.lastBounds.get(page.pageId) ?? null;
    if (next.width < 2 || next.height < 2) {
      await this.hideAll();
      return;
    }
    this.lastBounds.set(page.pageId, next);
    if (isBrowserPortalUrl(page.url)) {
      await this.hideAll();
      return;
    }
    if (!page.live) {
      await this.createPage(page.pageId, page.url, next);
    } else if (!boundsEqual(previous, next)) {
      await api().browserSetBounds?.({ pageId: page.pageId, bounds: next });
    }
    if (visible && !this.overlayOpen) {
      await this.show(page.pageId);
    } else {
      await this.hideAll();
    }
  }

  scheduleBounds(pageId: string, bounds: BrowserBounds) {
    this.lastBounds.set(pageId, roundBounds(bounds));
    if (this.frame != null) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = null;
      const latest = this.lastBounds.get(pageId);
      if (!latest) return;
      void api().browserSetBounds?.({ pageId, bounds: latest });
    });
  }

  async show(pageId: string) {
    if (this.overlayOpen || this.suppressed) {
      await this.hideAll();
      return;
    }
    if (this.visiblePageId === pageId) return;
    const revision = this.visibilityRevision;
    if (this.hidePromise) await this.hidePromise;
    if (revision !== this.visibilityRevision || this.overlayOpen || this.suppressed) return;
    await api().browserShowPage?.({ pageId });
    if (revision !== this.visibilityRevision || this.overlayOpen || this.suppressed) {
      await this.hideAll();
      return;
    }
    this.visiblePageId = pageId;
  }

  async hideAll() {
    if (this.visiblePageId == null && !browserSessionStore.livePageIds().length) return;
    this.visiblePageId = null;
    this.visibilityRevision += 1;
    const hiding = Promise.resolve(api().browserHideAll?.()).then(() => undefined);
    this.hidePromise = hiding;
    try {
      await hiding;
    } finally {
      if (this.hidePromise === hiding) this.hidePromise = null;
    }
  }

  suppress() {
    this.suppressed = true;
    this.visibilityRevision += 1;
    this.visiblePageId = null;
    void this.hideAll();
  }

  resume() {
    this.suppressed = false;
    this.visibilityRevision += 1;
    this.visibilityListener?.();
  }

  async discard(pageIds: string[]) {
    for (const pageId of pageIds) {
      await this.pendingCreates.get(pageId);
      this.lastBounds.delete(pageId);
      await api().browserClosePage?.({ pageId });
    }
    if (pageIds.includes(this.visiblePageId ?? '')) this.visiblePageId = null;
    browserSessionStore.markDiscarded(pageIds);
  }

  async setViewMode(pageId: string, viewMode: BrowserViewMode) {
    const current = browserSessionStore.page(pageId);
    if (!current || current.viewMode === viewMode) return;
    const previous = current.viewMode;
    browserSessionStore.setViewMode(pageId, viewMode);
    if (!current.live) return;
    try {
      await api().browserSetViewMode?.({ pageId, viewMode });
      browserSessionStore.markLoading(pageId, current.url !== BLANK_BROWSER_URL);
    } catch (error) {
      browserSessionStore.setViewMode(pageId, previous);
      throw error;
    }
  }

  async discardAll() {
    this.suppress();
    const live = browserSessionStore.livePageIds();
    await Promise.all(this.pendingCreates.values());
    await api().browserDiscardAll?.();
    this.lastBounds.clear();
    this.visiblePageId = null;
    browserSessionStore.markDiscarded(live);
  }

  async commitNavigation(pageId: string, url: string) {
    await this.ensureStarted();
    browserSessionStore.commitPageUrl(pageId, url);
    const page = browserSessionStore.page(pageId);
    if (!page) return;
    const pending = this.pendingCreates.get(pageId);
    if (pending) await pending;
    if (browserSessionStore.page(pageId)?.live) {
      await api().browserNavigate?.({ pageId, url: page.url });
      return;
    }
    await this.createPage(
      pageId,
      page.url,
      this.lastBounds.get(pageId) ?? { x: 0, y: 0, width: 1, height: 1 },
    );
  }

  async navigate(pageId: string, url: string) {
    await this.commitNavigation(pageId, url);
  }

  async goBack(pageId: string) {
    await api().browserGoBack?.({ pageId });
  }

  async goForward(pageId: string) {
    await api().browserGoForward?.({ pageId });
  }

  async reload(pageId: string) {
    await api().browserReload?.({ pageId });
    browserSessionStore.markLoading(pageId, true);
  }

  async stop(pageId: string) {
    await api().browserStop?.({ pageId });
    browserSessionStore.markLoading(pageId, false);
  }

  setOverlayOpen(open: boolean) {
    if (this.overlayOpen === open) return;
    this.overlayOpen = open;
    if (open) void this.hideAll();
    this.visibilityListener?.();
  }

  hasBlockingOverlay() {
    if (typeof document === 'undefined') return false;
    const isOpenOverlay = (element: Element) => element.getAttribute('data-state') !== 'closed';
    const containsOpenOverlay = (element: Element) => {
      if (element.matches(OVERLAY_SLOT_SELECTORS) && isOpenOverlay(element)) return true;
      return Array.from(element.querySelectorAll(OVERLAY_SLOT_SELECTORS)).some(isOpenOverlay);
    };
    const host = document.getElementById(overlayPortalHostId);
    if (host) {
      for (const child of Array.from(host.children)) {
        if (child.querySelector('[data-right-workspace-presentation="sheet"]')) continue;
        if (containsOpenOverlay(child)) return true;
        if (child.getAttribute('data-slot')?.includes('overlay') && isOpenOverlay(child)) return true;
      }
    }
    return Array.from(document.querySelectorAll(OVERLAY_SLOT_SELECTORS)).some(isOpenOverlay);
  }

  private watchOverlays() {
    if (typeof document === 'undefined' || this.overlayObserver) return;
    const sync = () => {
      this.setOverlayOpen(this.hasBlockingOverlay());
    };
    this.overlayObserver = new MutationObserver(sync);
    this.overlayObserver.observe(document.body, { childList: true, subtree: true });
    const host = document.getElementById(overlayPortalHostId);
    if (host) this.overlayObserver.observe(host, { childList: true, subtree: true });
    sync();
  }

  onVisibilityChange(listener: () => void) {
    this.visibilityListener = listener;
    return () => {
      if (this.visibilityListener === listener) this.visibilityListener = null;
    };
  }

  onOverlayChange(listener: () => void) {
    return this.onVisibilityChange(listener);
  }

  resetForTests() {
    if (this.frame != null) cancelAnimationFrame(this.frame);
    this.frame = null;
    this.overlayObserver?.disconnect();
    this.overlayObserver = null;
    this.unlisten?.();
    this.unlisten = null;
    this.started = false;
    this.startPromise = null;
    this.lastBounds.clear();
    this.pendingCreates.clear();
    this.hidePromise = null;
    this.visiblePageId = null;
    this.overlayOpen = false;
    this.visibilityListener = null;
    this.visibilityRevision = 0;
    this.suppressed = false;
  }

  private async createPage(pageId: string, url: string, bounds: BrowserBounds) {
    const existing = this.pendingCreates.get(pageId);
    if (existing) return existing;
    const create = (async () => {
      try {
        const evict = browserSessionStore.evictionCandidate(pageId);
        if (evict) await this.discard([evict]);
        await api().browserCreatePage?.({
          pageId,
          url,
          bounds: roundBounds(bounds),
          viewMode: browserSessionStore.page(pageId)?.viewMode ?? 'desktop',
        });
        if (!browserSessionStore.page(pageId)) {
          await api().browserClosePage?.({ pageId });
          return;
        }
        browserSessionStore.markLive(pageId, true);
        browserSessionStore.markLoading(pageId, url !== 'about:blank');
        const latest = this.lastBounds.get(pageId);
        if (latest) {
          await api().browserSetBounds?.({ pageId, bounds: latest });
        }
      } catch (error) {
        if (browserSessionStore.page(pageId)) {
          browserSessionStore.markLoading(pageId, false);
        }
        throw error;
      }
    })();
    this.pendingCreates.set(pageId, create);
    try {
      await create;
    } finally {
      if (this.pendingCreates.get(pageId) === create) this.pendingCreates.delete(pageId);
    }
  }
}

export const browserWebviewHost = new BrowserWebviewHost();
export { OVERLAY_SLOT_SELECTORS };
