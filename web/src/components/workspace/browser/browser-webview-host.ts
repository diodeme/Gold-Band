import { getRuntimeApi } from '@/api/client';
import { overlayOwnerAttribute, overlayPortalHostId, overlayCollisionBoundaryAttribute, conversationOverlayCollisionBoundary, rightWorkspaceOverlayOwner } from '@/lib/portal-container';
import {
  browserSessionStore,
  type BrowserPage,
  type BrowserViewMode,
  BLANK_BROWSER_URL,
  canonicalizeBrowserUrl,
  isBrowserPortalUrl,
} from './browser-session-store';

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
  '[data-slot="sheet-overlay"]',
  '[data-slot="dropdown-menu-content-positioner"]',
  '[data-slot="dropdown-menu-content"]',
  '[data-slot="dropdown-menu-sub-content-positioner"]',
  '[data-slot="dropdown-menu-sub-content"]',
  '[data-slot="context-menu-content-positioner"]',
  '[data-slot="context-menu-content"]',
  '[data-slot="context-menu-sub-content-positioner"]',
  '[data-slot="context-menu-sub-content"]',
  '[data-slot="popover-content"]',
  '[data-slot="select-content"]',
].join(',');

function overlayMeasurementTarget(element: Element): Element {
  const parent = element.parentElement;
  if (parent?.getAttribute('data-slot')?.endsWith('-content-positioner')) return parent;
  return element;
}

function readElementBounds(element: Element): BrowserBounds | null {
  const target = overlayMeasurementTarget(element);
  if (!(target instanceof HTMLElement)) return null;
  const visual = target.getBoundingClientRect();
  const width = Math.max(visual.width, target.offsetWidth);
  const height = Math.max(visual.height, target.offsetHeight);
  if (width < 1 || height < 1) return null;
  return {
    x: visual.x,
    y: visual.y,
    width,
    height,
  };
}

function overlayIsOpen(element: Element): boolean {
  const state = element.getAttribute('data-state');
  if (state === 'closed') return false;
  if (state === 'open') return true;
  const parentState = element.parentElement?.getAttribute('data-state');
  if (parentState === 'closed') return false;
  return parentState === 'open';
}

function rectsIntersect(left: BrowserBounds, right: BrowserBounds) {
  return left.x < right.x + right.width
    && left.x + left.width > right.x
    && left.y < right.y + right.height
    && left.y + left.height > right.y;
}

function isConversationConstrained(element: Element): boolean {
  const target = overlayMeasurementTarget(element);
  return target.getAttribute(overlayCollisionBoundaryAttribute) === conversationOverlayCollisionBoundary
    || element.getAttribute(overlayCollisionBoundaryAttribute) === conversationOverlayCollisionBoundary;
}

function isRightWorkspaceExempt(element: Element): boolean {
  const host = document.getElementById(overlayPortalHostId);
  if (!host) return false;
  for (const child of Array.from(host.children)) {
    const owned = child.getAttribute(overlayOwnerAttribute) === rightWorkspaceOverlayOwner
      || Boolean(child.querySelector('[data-right-workspace-presentation="sheet"]'));
    if (owned && (child === element || child.contains(element))) return true;
  }
  return false;
}

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
  private lastBounds = new Map<string, BrowserBounds>();
  private visiblePageId: string | null = null;
  private overlayOpen = false;
  private overlayObserver: MutationObserver | null = null;
  private overlayResizeObserver: ResizeObserver | null = null;
  private visibilityListener: (() => void) | null = null;
  private layoutListener: (() => void) | null = null;
  private readonly pendingCreates = new Map<string, Promise<void>>();
  private readonly pendingNavigations = new Map<string, {
    target: string;
    queued: string | null;
    promise: Promise<void>;
  }>();
  private readonly pendingBounds = new Map<string, {
    target: BrowserBounds;
    queued: BrowserBounds | null;
    promise: Promise<void>;
  }>();
  private hidePromise: Promise<void> | null = null;
  private showPromise: Promise<void> | null = null;
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
      if (!this.suppressed) await this.hideAll();
      return;
    }
    this.lastBounds.set(page.pageId, next);
    this.setOverlayOpen(this.hasBlockingOverlay());
    if (isBrowserPortalUrl(page.url)) {
      await this.hideAll();
      return;
    }
    if (!page.live) {
      try {
        await this.createPage(page.pageId, page.url, next);
      } catch (error) {
        await this.alignRetainedPage(page.pageId, previous, next, visible);
        throw error;
      }
    } else if (!this.suppressed && !boundsEqual(previous, next)) {
      await this.commitBounds(page.pageId, next);
    }
    if (visible && !this.overlayOpen) {
      await this.show(page.pageId);
    } else {
      await this.hideAll();
    }
  }

  async show(pageId: string) {
    if (this.overlayOpen || this.suppressed) {
      await this.hideAll();
      return;
    }
    if (this.visiblePageId === pageId) return;
    const revision = this.visibilityRevision;
    if (this.showPromise) {
      await this.showPromise;
      if (this.visiblePageId === pageId) return;
    }
    if (this.hidePromise) await this.hidePromise;
    if (revision !== this.visibilityRevision || this.overlayOpen || this.suppressed) return;
    const showing = (async () => {
      await api().browserShowPage?.({ pageId });
      if (revision !== this.visibilityRevision || this.overlayOpen || this.suppressed) {
        await this.hideAll();
        return;
      }
      this.visiblePageId = pageId;
    })();
    this.showPromise = showing;
    try {
      await showing;
    } finally {
      if (this.showPromise === showing) this.showPromise = null;
    }
  }

  async hideAll() {
    if (this.hidePromise) return this.hidePromise;
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

  async suppress() {
    const changed = !this.suppressed;
    this.suppressed = true;
    if (changed) this.visibilityRevision += 1;
    this.visiblePageId = null;
    await this.hideAll();
  }

  resume() {
    if (!this.suppressed) return;
    this.suppressed = false;
    this.visibilityRevision += 1;
    this.visibilityListener?.();
  }

  async discard(pageIds: string[]) {
    for (const pageId of pageIds) {
      await this.pendingCreates.get(pageId);
      await this.pendingBounds.get(pageId)?.promise;
      this.lastBounds.delete(pageId);
      this.pendingBounds.delete(pageId);
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
    await this.suppress();
    const live = browserSessionStore.livePageIds();
    await Promise.all(this.pendingCreates.values());
    await Promise.all([...this.pendingBounds.values()].map((operation) => operation.promise));
    await api().browserDiscardAll?.();
    this.lastBounds.clear();
    this.pendingBounds.clear();
    this.visiblePageId = null;
    browserSessionStore.markDiscarded(live);
  }

  commitNavigation(pageId: string, url: string) {
    const target = canonicalizeBrowserUrl(url);
    const pending = this.pendingNavigations.get(pageId);
    if (pending) {
      if (pending.target !== target) pending.queued = target;
      return pending.promise;
    }
    const operation = {
      target,
      queued: null as string | null,
      promise: Promise.resolve(),
    };
    operation.promise = this.runNavigationQueue(pageId, operation);
    this.pendingNavigations.set(pageId, operation);
    return operation.promise;
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
    const viewport = this.viewportBounds();
    if (!viewport) return false;
    return this.listOpenBlockingOverlays().some((element) => {
      const overlayBounds = readElementBounds(element);
      return overlayBounds != null && rectsIntersect(overlayBounds, viewport);
    });
  }

  private viewportBounds(): BrowserBounds | null {
    if (typeof document !== 'undefined') {
      const host = document.querySelector('[data-browser-native-host]');
      if (host instanceof HTMLElement) {
        const rect = host.getBoundingClientRect();
        const width = Math.max(rect.width, host.offsetWidth);
        const height = Math.max(rect.height, host.offsetHeight);
        if (width >= 2 && height >= 2) {
          return roundBounds({ x: rect.x, y: rect.y, width, height });
        }
      }
    }
    if (this.visiblePageId) {
      const bounds = this.lastBounds.get(this.visiblePageId);
      if (bounds) return bounds;
    }
    for (const bounds of this.lastBounds.values()) {
      if (bounds.width >= 2 && bounds.height >= 2) return bounds;
    }
    return null;
  }

  private listOpenBlockingOverlays(): Element[] {
    if (typeof document === 'undefined') return [];
    return Array.from(document.querySelectorAll(OVERLAY_SLOT_SELECTORS)).filter((element) => (
      overlayIsOpen(element) && !isRightWorkspaceExempt(element) && !isConversationConstrained(element)
    ));
  }

  private refreshOverlayResizeTargets() {
    if (!this.overlayResizeObserver) return;
    this.overlayResizeObserver.disconnect();
    const host = document.querySelector('[data-browser-native-host]');
    if (host) this.overlayResizeObserver.observe(host);
    for (const overlay of this.listOpenBlockingOverlays()) {
      this.overlayResizeObserver.observe(overlay);
    }
  }

  private watchOverlays() {
    if (typeof document === 'undefined' || this.overlayObserver) return;
    const sync = () => this.setOverlayOpen(this.hasBlockingOverlay());
    this.overlayObserver = new MutationObserver(() => {
      sync();
      this.refreshOverlayResizeTargets();
    });
    this.overlayObserver.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['data-state', 'data-slot', overlayOwnerAttribute, overlayCollisionBoundaryAttribute],
    });
    const host = document.getElementById(overlayPortalHostId);
    if (host) {
      this.overlayObserver.observe(host, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['data-state', 'data-slot', overlayOwnerAttribute, overlayCollisionBoundaryAttribute],
      });
    }
    if (typeof ResizeObserver !== 'undefined') {
      this.overlayResizeObserver = new ResizeObserver(sync);
    }
    sync();
    this.refreshOverlayResizeTargets();
  }

  onVisibilityChange(listener: () => void) {
    this.visibilityListener = listener;
    return () => {
      if (this.visibilityListener === listener) this.visibilityListener = null;
    };
  }

  onLayoutFrame(listener: () => void) {
    this.layoutListener = listener;
    return () => {
      if (this.layoutListener === listener) this.layoutListener = null;
    };
  }

  notifyLayoutFrame() {
    this.layoutListener?.();
  }

  onOverlayChange(listener: () => void) {
    return this.onVisibilityChange(listener);
  }

  resetForTests() {
    this.overlayObserver?.disconnect();
    this.overlayObserver = null;
    this.overlayResizeObserver?.disconnect();
    this.overlayResizeObserver = null;
    this.unlisten?.();
    this.unlisten = null;
    this.started = false;
    this.startPromise = null;
    this.lastBounds.clear();
    this.pendingCreates.clear();
    this.pendingNavigations.clear();
    this.pendingBounds.clear();
    this.hidePromise = null;
    this.showPromise = null;
    this.visiblePageId = null;
    this.overlayOpen = false;
    this.visibilityListener = null;
    this.layoutListener = null;
    this.visibilityRevision = 0;
    this.suppressed = false;
  }

  private async alignRetainedPage(
    pageId: string,
    previous: BrowserBounds | null,
    next: BrowserBounds,
    visible: boolean,
  ) {
    if (this.suppressed || boundsEqual(previous, next)) return;
    try {
      await this.commitBounds(pageId, next);
    } catch {
      // Create already failed. Moving the retained window is best-effort when one still exists.
    }
    if (!visible || this.overlayOpen) return;
    try {
      await this.show(pageId);
    } catch {
      // A missing native page stays hidden; the navigation notice already records the failure.
    }
  }

  private async createPage(pageId: string, url: string, bounds: BrowserBounds) {
    const existing = this.pendingCreates.get(pageId);
    if (existing) return existing;
    const create = (async () => {
      try {
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
        if (latest && !this.suppressed) {
          await this.commitBounds(pageId, latest);
        }
      } catch (error) {
        if (browserSessionStore.page(pageId)) {
          browserSessionStore.failNavigation(pageId, browserErrorCode(error));
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

  private commitBounds(pageId: string, bounds: BrowserBounds) {
    const target = roundBounds(bounds);
    const pending = this.pendingBounds.get(pageId);
    if (pending) {
      pending.queued = target;
      return pending.promise;
    }
    const operation = {
      target,
      queued: null as BrowserBounds | null,
      promise: Promise.resolve(),
    };
    operation.promise = this.runBoundsQueue(pageId, operation);
    this.pendingBounds.set(pageId, operation);
    return operation.promise;
  }

  private async runBoundsQueue(
    pageId: string,
    operation: { target: BrowserBounds; queued: BrowserBounds | null; promise: Promise<void> },
  ) {
    try {
      let target: BrowserBounds | null = operation.target;
      while (target) {
        operation.target = target;
        if (!this.suppressed) {
          await api().browserSetBounds?.({ pageId, bounds: target });
        }
        const queued = operation.queued;
        operation.queued = null;
        target = queued && !boundsEqual(queued, target) ? queued : null;
      }
    } finally {
      if (this.pendingBounds.get(pageId) === operation) this.pendingBounds.delete(pageId);
    }
  }

  private async runNavigationQueue(
    pageId: string,
    operation: { target: string; queued: string | null; promise: Promise<void> },
  ) {
    try {
      let target: string | null = operation.target;
      while (target) {
        operation.target = target;
        await this.performNavigation(pageId, target);
        const queued = operation.queued;
        operation.queued = null;
        target = queued && queued !== target ? queued : null;
      }
    } finally {
      if (this.pendingNavigations.get(pageId) === operation) {
        this.pendingNavigations.delete(pageId);
      }
    }
  }

  private async performNavigation(pageId: string, target: string) {
    await this.ensureStarted();
    const current = browserSessionStore.page(pageId);
    if (!current) return;
    if (current.live && canonicalizeBrowserUrl(current.url) === target) return;
    browserSessionStore.beginNavigation(pageId);
    try {
      const pendingCreate = this.pendingCreates.get(pageId);
      if (pendingCreate) await pendingCreate;
      const latest = browserSessionStore.page(pageId);
      if (!latest) return;
      if (latest.live) {
        const result = await api().browserNavigate?.({ pageId, url: target });
        browserSessionStore.commitPageUrl(pageId, result?.url ?? target);
        return;
      }
      await this.createPage(
        pageId,
        target,
        this.lastBounds.get(pageId) ?? { x: 0, y: 0, width: 1, height: 1 },
      );
      browserSessionStore.commitPageUrl(pageId, target);
    } catch (error) {
      browserSessionStore.failNavigation(pageId, browserErrorCode(error));
      throw error;
    }
  }
}

function browserErrorCode(error: unknown) {
  if (typeof error === 'object' && error && 'code' in error && typeof error.code === 'string') {
    return error.code;
  }
  return 'browser.webview.unavailable';
}

export const browserWebviewHost = new BrowserWebviewHost();
export { OVERLAY_SLOT_SELECTORS };
