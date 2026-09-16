export const BROWSER_PAGE_LIMIT = 32;
export const BROWSER_LIVE_WEBVIEW_LIMIT = 5;
export const BROWSER_LOAD_STALL_MS = 15_000;
export const BLANK_BROWSER_URL = 'about:blank';

export type BrowserViewMode = 'desktop' | 'mobile';

export interface BrowserPage {
  pageId: string;
  url: string;
  title: string;
  loading: boolean;
  live: boolean;
  viewMode: BrowserViewMode;
}

export interface BrowserSessionState {
  pages: BrowserPage[];
  activePageId: string | null;
  noticeCode: string | null;
}

export interface BrowserPageLimitError {
  code: 'browser.page.limit_reached';
  params: Record<string, never>;
}

type BrowserSessionListener = () => void;

function createPageId() {
  return globalThis.crypto.randomUUID().replaceAll('-', '');
}

export function canonicalizeBrowserUrl(raw: string) {
  const trimmed = raw.trim();
  if (!trimmed || trimmed.toLowerCase() === BLANK_BROWSER_URL) return BLANK_BROWSER_URL;
  try {
    const url = new URL(trimmed);
    if (url.protocol === 'http:' || url.protocol === 'https:') {
      url.hash = '';
      return url.toString();
    }
    if (url.protocol === 'file:') return url.href;
    return trimmed;
  } catch {
    return trimmed;
  }
}

export function isReusableBrowserUrl(url: string) {
  return canonicalizeBrowserUrl(url) !== BLANK_BROWSER_URL;
}

export function isBrowserPortalUrl(url: string) {
  return canonicalizeBrowserUrl(url) === BLANK_BROWSER_URL;
}

function cloneState(state: BrowserSessionState): BrowserSessionState {
  return {
    activePageId: state.activePageId,
    noticeCode: state.noticeCode,
    pages: state.pages.map((page) => ({ ...page })),
  };
}

export class BrowserSessionStore {
  private state: BrowserSessionState = { pages: [], activePageId: null, noticeCode: null };
  private readonly listeners = new Set<BrowserSessionListener>();
  private readonly loadStallTimers = new Map<string, ReturnType<typeof setTimeout>>();

  snapshot() {
    return this.state;
  }

  subscribe(listener: BrowserSessionListener) {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  openUrl(url: string, title?: string) {
    const canonical = canonicalizeBrowserUrl(url);
    if (isReusableBrowserUrl(canonical)) {
      const existing = this.state.pages.find((page) => canonicalizeBrowserUrl(page.url) === canonical);
      if (existing) {
        this.activate(existing.pageId);
        return existing.pageId;
      }
    }
    if (this.state.pages.length >= BROWSER_PAGE_LIMIT) {
      const error: BrowserPageLimitError = { code: 'browser.page.limit_reached', params: {} };
      throw error;
    }
    const page: BrowserPage = {
      pageId: createPageId(),
      url: canonical,
      title: title?.trim() || '',
      loading: canonical !== BLANK_BROWSER_URL,
      live: false,
      viewMode: 'desktop',
    };
    this.state = {
      pages: [...this.state.pages, page],
      activePageId: page.pageId,
      noticeCode: null,
    };
    if (canonical !== BLANK_BROWSER_URL) this.scheduleLoadStallRecovery(page.pageId);
    this.emit();
    return page.pageId;
  }

  addBlankPage() {
    return this.openUrl(BLANK_BROWSER_URL);
  }

  setViewMode(pageId: string, viewMode: BrowserViewMode) {
    const page = this.page(pageId);
    if (!page || page.viewMode === viewMode) return page;
    this.patch(pageId, { viewMode });
    return this.page(pageId);
  }

  activate(pageId: string) {
    if (this.state.activePageId === pageId || !this.state.pages.some((page) => page.pageId === pageId)) {
      return;
    }
    this.state = { ...this.state, activePageId: pageId };
    this.emit();
  }

  close(pageId: string) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return [];
    this.clearLoadStallTimer(pageId);
    const pages = this.state.pages.filter((page) => page.pageId !== pageId);
    const activePageId = this.state.activePageId === pageId
      ? (pages[Math.min(index, pages.length - 1)]?.pageId ?? null)
      : this.state.activePageId;
    this.state = { ...this.state, pages, activePageId };
    this.emit();
    return [pageId];
  }

  closeOthers(pageId: string) {
    if (!this.state.pages.some((page) => page.pageId === pageId)) return [];
    const closed = this.state.pages.filter((page) => page.pageId !== pageId).map((page) => page.pageId);
    for (const closedPageId of closed) this.clearLoadStallTimer(closedPageId);
    this.state = {
      ...this.state,
      pages: this.state.pages.filter((page) => page.pageId === pageId),
      activePageId: pageId,
    };
    this.emit();
    return closed;
  }

  closeToTheLeft(pageId: string) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index <= 0) return [];
    const closed = this.state.pages.slice(0, index).map((page) => page.pageId);
    for (const closedPageId of closed) this.clearLoadStallTimer(closedPageId);
    const pages = this.state.pages.slice(index);
    this.state = {
      ...this.state,
      pages,
      activePageId: pages.some((page) => page.pageId === this.state.activePageId)
        ? this.state.activePageId
        : pageId,
    };
    this.emit();
    return closed;
  }

  closeToTheRight(pageId: string) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0 || index === this.state.pages.length - 1) return [];
    const closed = this.state.pages.slice(index + 1).map((page) => page.pageId);
    for (const closedPageId of closed) this.clearLoadStallTimer(closedPageId);
    const pages = this.state.pages.slice(0, index + 1);
    this.state = {
      ...this.state,
      pages,
      activePageId: pages.some((page) => page.pageId === this.state.activePageId)
        ? this.state.activePageId
        : pageId,
    };
    this.emit();
    return closed;
  }

  closeAll() {
    const closed = this.state.pages.map((page) => page.pageId);
    for (const pageId of closed) this.clearLoadStallTimer(pageId);
    this.state = { pages: [], activePageId: null, noticeCode: null };
    this.emit();
    return closed;
  }

  markLive(pageId: string, live: boolean) {
    this.patch(pageId, { live, loading: live ? this.page(pageId)?.loading ?? false : false });
  }

  markLoading(pageId: string, loading: boolean) {
    this.patch(pageId, { loading });
    if (loading) this.scheduleLoadStallRecovery(pageId);
    else this.clearLoadStallTimer(pageId);
  }

  commitPageUrl(pageId: string, url: string) {
    const canonical = canonicalizeBrowserUrl(url);
    this.patch(pageId, { url: canonical, loading: canonical !== BLANK_BROWSER_URL });
    if (canonical === BLANK_BROWSER_URL) this.clearLoadStallTimer(pageId);
    else this.scheduleLoadStallRecovery(pageId);
  }

  applyNativeEvent(event: { kind: string; pageId: string; url?: string | null; title?: string | null }) {
    if (event.kind === 'title' && event.title) {
      this.patch(event.pageId, { title: event.title });
      return;
    }
    if (event.kind === 'url' && event.url) {
      this.patch(event.pageId, { url: event.url });
      return;
    }
    if (event.kind === 'load-start') {
      this.patch(event.pageId, {
        loading: true,
        ...(event.url ? { url: event.url } : {}),
      });
      this.scheduleLoadStallRecovery(event.pageId);
      return;
    }
    if (event.kind === 'download-unsupported' || event.kind === 'download-cancelled') {
      this.state = {
        ...this.state,
        noticeCode: event.kind === 'download-cancelled'
          ? 'browser.download.cancelled'
          : 'browser.download.unsupported',
      };
      this.emit();
      return;
    }
    if (event.kind === 'load-finish') {
      this.clearLoadStallTimer(event.pageId);
      this.patch(event.pageId, {
        loading: false,
        live: true,
        ...(event.url ? { url: event.url } : {}),
      });
    }
  }

  livePageIds() {
    return this.state.pages.filter((page) => page.live).map((page) => page.pageId);
  }

  evictionCandidate(keepPageId: string | null) {
    const live = this.state.pages.filter((page) => page.live);
    if (live.length < BROWSER_LIVE_WEBVIEW_LIMIT) return null;
    return live.find((page) => page.pageId !== keepPageId)?.pageId ?? null;
  }

  markDiscarded(pageIds: string[]) {
    if (pageIds.length === 0) return;
    const discarded = new Set(pageIds);
    for (const pageId of discarded) this.clearLoadStallTimer(pageId);
    this.state = {
      ...this.state,
      pages: this.state.pages.map((page) => (
        discarded.has(page.pageId)
          ? { ...page, live: false, loading: false }
          : page
      )),
    };
    this.emit();
  }

  page(pageId: string) {
    return this.state.pages.find((page) => page.pageId === pageId) ?? null;
  }

  activePage() {
    return this.state.activePageId ? this.page(this.state.activePageId) : null;
  }

  resetForTests() {
    for (const timer of this.loadStallTimers.values()) clearTimeout(timer);
    this.loadStallTimers.clear();
    this.state = { pages: [], activePageId: null, noticeCode: null };
    this.emit();
  }

  private scheduleLoadStallRecovery(pageId: string) {
    this.clearLoadStallTimer(pageId);
    const timer = setTimeout(() => {
      this.loadStallTimers.delete(pageId);
      if (this.page(pageId)?.loading) this.patch(pageId, { loading: false });
    }, BROWSER_LOAD_STALL_MS);
    this.loadStallTimers.set(pageId, timer);
  }

  private clearLoadStallTimer(pageId: string) {
    const timer = this.loadStallTimers.get(pageId);
    if (timer != null) clearTimeout(timer);
    this.loadStallTimers.delete(pageId);
  }

  private patch(pageId: string, patch: Partial<BrowserPage>) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return;
    const pages = this.state.pages.map((page, pageIndex) => (
      pageIndex === index ? { ...page, ...patch } : page
    ));
    this.state = { ...this.state, pages };
    this.emit();
  }

  private emit() {
    this.state = cloneState(this.state);
    for (const listener of this.listeners) listener();
  }
}

export const browserSessionStore = new BrowserSessionStore();
