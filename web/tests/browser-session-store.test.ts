import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  BROWSER_LIVE_WEBVIEW_LIMIT,
  BROWSER_LOAD_STALL_MS,
  BROWSER_PAGE_LIMIT,
  BLANK_BROWSER_URL,
  BrowserSessionStore,
  canonicalizeBrowserUrl,
  isBrowserPortalUrl,
} from '@/components/workspace/browser/browser-session-store';

describe('browser session store', () => {
  const store = new BrowserSessionStore();

  afterEach(() => {
    store.resetForTests();
  });

  it('reuses a normalized http page instead of creating another tab', () => {
    const first = store.openUrl('https://example.com/docs');
    const second = store.openUrl('https://example.com/docs');
    expect(second).toBe(first);
    expect(store.snapshot().pages).toHaveLength(1);
    expect(canonicalizeBrowserUrl('https://example.com/docs#section')).toBe('https://example.com/docs');
  });

  it('does not reuse blank pages', () => {
    const first = store.addBlankPage();
    const second = store.addBlankPage();
    expect(second).not.toBe(first);
    expect(store.snapshot().pages).toHaveLength(2);
    expect(store.page(first)?.url).toBe(BLANK_BROWSER_URL);
    expect(isBrowserPortalUrl(BLANK_BROWSER_URL)).toBe(true);
    expect(isBrowserPortalUrl('https://github.com/')).toBe(false);
  });

  it('rejects a 33rd page with a structured error', () => {
    for (let index = 0; index < BROWSER_PAGE_LIMIT; index += 1) {
      store.openUrl(`https://example.com/page-${index}`);
    }
    let thrown: unknown;
    try {
      store.openUrl('https://example.com/overflow');
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toEqual({ code: 'browser.page.limit_reached', params: {} });
    expect(store.snapshot().pages).toHaveLength(BROWSER_PAGE_LIMIT);
  });

  it('keeps close-all from wiping the session object and only clears pages', () => {
    store.openUrl('https://one.example');
    store.openUrl('https://two.example');
    expect(store.closeAll()).toHaveLength(2);
    expect(store.snapshot()).toEqual({ pages: [], activePageId: null, noticeCode: null });
  });

  it('commits an address onto an existing blank page', () => {
    const pageId = store.addBlankPage();
    store.commitPageUrl(pageId, 'https://baidu.com');
    expect(store.page(pageId)?.url).toBe('https://baidu.com/');
    expect(store.page(pageId)?.loading).toBe(true);
  });

  it('defaults new pages to desktop and keeps view mode on the page identity', () => {
    const pageId = store.openUrl('https://example.com');
    expect(store.page(pageId)?.viewMode).toBe('desktop');
    store.setViewMode(pageId, 'mobile');
    expect(store.page(pageId)?.viewMode).toBe('mobile');
    store.setViewMode(pageId, 'mobile');
    expect(store.page(pageId)?.viewMode).toBe('mobile');
    store.setViewMode(pageId, 'desktop');
    expect(store.page(pageId)?.viewMode).toBe('desktop');
  });

  it('starts the load stall timer when opening a remote page', () => {
    vi.useFakeTimers();
    try {
      const pageId = store.openUrl('https://example.com/');
      expect(store.page(pageId)?.loading).toBe(true);
      vi.advanceTimersByTime(BROWSER_LOAD_STALL_MS);
      expect(store.page(pageId)?.loading).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('clears a hung load so the native page can be shown again', () => {
    vi.useFakeTimers();
    try {
      const pageId = store.openUrl('https://baidu/');
      expect(store.page(pageId)?.loading).toBe(true);
      store.applyNativeEvent({ kind: 'load-start', pageId, url: 'https://baidu/' });
      vi.advanceTimersByTime(BROWSER_LOAD_STALL_MS);
      expect(store.page(pageId)?.loading).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('evicts the oldest live webview that is not the current page', () => {
    const ids: string[] = [];
    for (let index = 0; index < BROWSER_LIVE_WEBVIEW_LIMIT; index += 1) {
      ids.push(store.openUrl(`https://live.example/${index}`));
      store.markLive(ids[index]!, true);
    }
    const next = store.openUrl('https://live.example/new');
    expect(store.evictionCandidate(next)).toBe(ids[0]);
    expect(store.evictionCandidate(ids[0]!)).toBe(ids[1]);
  });
});
