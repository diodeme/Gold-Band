export const BROWSER_PAGE_LIMIT = 32;
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

export interface BrowserNotice {
  pageId: string;
  code: string;
  params: Record<string, unknown>;
}

export const BROWSER_LOCAL_ACCESS_DENIED_CODE = 'browser.local_html.access_denied';

export interface BrowserSessionState {
  pages: BrowserPage[];
  activePageId: string | null;
  notice: BrowserNotice | null;
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
    notice: state.notice ? { ...state.notice, params: { ...state.notice.params } } : null,
    pages: state.pages.map((page) => ({ ...page })),
  };
}

function isDownloadNotice(code: string) {
  return code === 'browser.download.cancelled' || code === 'browser.download.unsupported';
}

export class BrowserSessionStore {
  private state: BrowserSessionState = { pages: [], activePageId: null, notice: null };
  private readonly listeners = new Set<BrowserSessionListener>();
  private readonly loadStallTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private eventRevision = 0;
  private noticeRevision = 0;
  private readonly loadStartRevision = new Map<string, number>();

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
      notice: null,
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
    this.state = { pages: [], activePageId: null, notice: null };
    this.emit();
    return closed;
  }

  markLive(pageId: string, live: boolean) {
    this.patch(
      pageId,
      { live, loading: live ? this.page(pageId)?.loading ?? false : false },
      live && this.navigationNoticeExpired(pageId),
    );
  }

  markLoading(pageId: string, loading: boolean) {
    this.patch(pageId, { loading });
    if (loading) this.scheduleLoadStallRecovery(pageId);
    else this.clearLoadStallTimer(pageId);
  }

  beginNavigation(pageId: string) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return;
    const pages = this.state.pages.map((page, pageIndex) => (
      pageIndex === index ? { ...page, loading: true } : page
    ));
    this.state = { ...this.state, pages, notice: null };
    this.scheduleLoadStallRecovery(pageId);
    this.emit();
  }

  commitPageUrl(pageId: string, url: string) {
    const canonical = canonicalizeBrowserUrl(url);
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return;
    const pages = this.state.pages.map((page, pageIndex) => (
      pageIndex === index
        ? { ...page, url: canonical, loading: canonical !== BLANK_BROWSER_URL }
        : page
    ));
    this.state = { ...this.state, pages, notice: null };
    this.emit();
    if (canonical === BLANK_BROWSER_URL) this.clearLoadStallTimer(pageId);
    else this.scheduleLoadStallRecovery(pageId);
  }

  failNavigation(pageId: string, code: string) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return;
    this.clearLoadStallTimer(pageId);
    const pages = this.state.pages.map((page, pageIndex) => (
      pageIndex === index ? { ...page, loading: false } : page
    ));
    this.showNotice({ pageId, code, params: {} }, pages);
  }

  /** Surfaces a page-scoped failure that is not a navigation, such as the system browser refusing it. */
  reportNotice(pageId: string, code: string) {
    if (!this.page(pageId)) return;
    this.showNotice({ pageId, code, params: {} }, this.state.pages);
  }

  /** Drops the page's notice once the user acted on it, such as allowing local access. */
  dismissNotice(pageId: string) {
    if (this.state.notice?.pageId !== pageId) return;
    this.state = { ...this.state, notice: null };
    this.emit();
  }

  private showNotice(notice: BrowserNotice, pages: BrowserPage[]) {
    this.eventRevision += 1;
    this.noticeRevision = this.eventRevision;
    this.state = { ...this.state, pages, notice };
    this.emit();
  }

  applyNativeEvent(event: {
    kind: string;
    pageId: string;
    url?: string | null;
    title?: string | null;
    directories?: string[];
  }) {
    if (event.kind === 'discarded') {
      this.markDiscarded([event.pageId]);
      return;
    }
    if (event.kind === 'title' && event.title) {
      this.patch(event.pageId, { title: event.title });
      return;
    }
    if (event.kind === 'url' && event.url) {
      this.patch(event.pageId, { url: event.url });
      return;
    }
    if (event.kind === 'load-start') {
      this.eventRevision += 1;
      this.loadStartRevision.set(event.pageId, this.eventRevision);
      this.patch(event.pageId, {
        loading: true,
        ...(event.url ? { url: event.url } : {}),
      }, this.localAccessNoticeLeftBehind(event.pageId, event.url));
      this.scheduleLoadStallRecovery(event.pageId);
      return;
    }
    if (event.kind === 'local-access-denied') {
      if (!this.page(event.pageId) || !event.directories?.length) return;
      this.showNotice({
        pageId: event.pageId,
        code: BROWSER_LOCAL_ACCESS_DENIED_CODE,
        params: { directories: [...event.directories] },
      }, this.state.pages);
      return;
    }
    if (event.kind === 'download-unsupported' || event.kind === 'download-cancelled') {
      this.state = {
        ...this.state,
        notice: {
          pageId: event.pageId,
          code: event.kind === 'download-cancelled'
            ? 'browser.download.cancelled'
            : 'browser.download.unsupported',
          params: {},
        },
      };
      this.emit();
      return;
    }
    if (event.kind === 'load-finish') {
      this.clearLoadStallTimer(event.pageId);
      const started = this.loadStartRevision.get(event.pageId) ?? 0;
      this.patch(event.pageId, {
        loading: false,
        live: true,
        ...(event.url ? { url: event.url } : {}),
      }, this.navigationNoticeExpired(event.pageId, started));
    }
  }

  livePageIds() {
    return this.state.pages.filter((page) => page.live).map((page) => page.pageId);
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
    this.loadStartRevision.clear();
    this.eventRevision = 0;
    this.noticeRevision = 0;
    this.state = { pages: [], activePageId: null, notice: null };
    this.emit();
  }

  private navigationNoticeExpired(pageId: string, loadStartedAt?: number) {
    const notice = this.state.notice;
    if (notice?.pageId !== pageId) return false;
    // Refused local directories stay pending across reloads until the user answers.
    if (isDownloadNotice(notice.code) || notice.code === BROWSER_LOCAL_ACCESS_DENIED_CODE) return false;
    return loadStartedAt === undefined || loadStartedAt > this.noticeRevision;
  }

  /** The page left local documents, which also drops its pending directories natively. */
  private localAccessNoticeLeftBehind(pageId: string, url?: string | null) {
    const notice = this.state.notice;
    return notice?.pageId === pageId
      && notice.code === BROWSER_LOCAL_ACCESS_DENIED_CODE
      && typeof url === 'string'
      && !url.startsWith('file:');
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

  private patch(pageId: string, patch: Partial<BrowserPage>, clearNotice = false) {
    const index = this.state.pages.findIndex((page) => page.pageId === pageId);
    if (index < 0) return;
    const pages = this.state.pages.map((page, pageIndex) => (
      pageIndex === index ? { ...page, ...patch } : page
    ));
    const notice = clearNotice ? null : this.state.notice;
    this.state = { ...this.state, pages, notice };
    this.emit();
  }

  private emit() {
    this.state = cloneState(this.state);
    for (const listener of this.listeners) listener();
  }
}

export const browserSessionStore = new BrowserSessionStore();
