/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { browserSessionStore } from '@/components/workspace/browser/browser-session-store';

vi.mock('@/components/workspace/browser/NativeBrowserViewport', () => ({
  NativeBrowserViewport: () => <div data-browser-viewport="true" />,
}));

const bookmarks = vi.hoisted(() => ([
  {
    bookmarkId: 'github',
    url: 'https://github.com/',
    title: 'GitHub',
    origin: 'https://github.com',
    faviconDataUrl: 'data:image/png;base64,aaa',
  },
]));

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({
    browserListHistory: async () => [{
      url: 'https://github.com/diodeme/Gold-Band',
      title: 'GitHub',
      origin: 'https://github.com',
      lastVisitedAt: 1,
      faviconDataUrl: 'data:image/png;base64,aaa',
    }],
    browserListBookmarks: async () => bookmarks,
    browserAddBookmark: async (input: { url: string; title?: string | null }) => {
      bookmarks.push({
        bookmarkId: 'added',
        url: input.url,
        title: input.title || 'Added',
        origin: new URL(input.url).origin,
        faviconDataUrl: null,
      });
      return [...bookmarks];
    },
    browserRemoveBookmark: async (input: { bookmarkId: string }) => {
      const index = bookmarks.findIndex((item) => item.bookmarkId === input.bookmarkId);
      if (index >= 0) bookmarks.splice(index, 1);
      return [...bookmarks];
    },
    browserReorderBookmarks: async (input: { orderedIds: string[] }) => {
      bookmarks.sort((left, right) => input.orderedIds.indexOf(left.bookmarkId) - input.orderedIds.indexOf(right.bookmarkId));
      return [...bookmarks];
    },
    subscribeBrowserHistoryEvents: async () => () => undefined,
  }),
}));

vi.mock('@/components/workspace/browser/browser-webview-host', () => ({
  browserWebviewHost: {
    discard: vi.fn(),
    hideAll: vi.fn(),
    goBack: vi.fn(),
    goForward: vi.fn(),
    reload: vi.fn(),
    stop: vi.fn(),
    navigate: vi.fn(),
    commitNavigation: vi.fn(),
    setViewMode: vi.fn(),
    resume: vi.fn(),
    suppress: vi.fn(),
  },
}));

vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => children,
  TooltipContent: ({ children }: { children: React.ReactNode }) => <span data-browser-toolbar-tooltip="true">{children}</span>,
  TooltipProvider: ({ children }: { children: React.ReactNode }) => children,
}));

import { BrowserWorkspacePanel } from '@/components/workspace/browser/BrowserWorkspacePanel';
import { browserBookmarkStore } from '@/components/workspace/browser/browser-bookmark-store';
import { browserHistoryStore } from '@/components/workspace/browser/browser-history-store';
import { browserWebviewHost } from '@/components/workspace/browser/browser-webview-host';
import { openExternalUrl } from '@/api';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return {
    ...actual,
    openExternalUrl: vi.fn(async () => undefined),
  };
});

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserver);

afterEach(() => {
  browserSessionStore.resetForTests();
  browserHistoryStore.resetForTests();
  browserBookmarkStore.resetForTests();
  bookmarks.splice(0, bookmarks.length, {
    bookmarkId: 'github',
    url: 'https://github.com/',
    title: 'GitHub',
    origin: 'https://github.com',
    faviconDataUrl: 'data:image/png;base64,aaa',
  });
  document.body.replaceChildren();
});

describe('BrowserWorkspacePanel', () => {
  it('does not own native visibility lifecycle across workspace remounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<BrowserWorkspacePanel />));
      expect(browserWebviewHost.resume).not.toHaveBeenCalled();
      await act(async () => root.unmount());
      expect(browserWebviewHost.suppress).not.toHaveBeenCalled();
    } finally {
      container.remove();
    }
  });

  it('keeps the new-tab control after every internal page is closed', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<BrowserWorkspacePanel />));
      expect(container.querySelector('[data-browser-portal="true"]')).not.toBeNull();
      expect(container.querySelector('[data-browser-new-page="true"]')).not.toBeNull();
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-new-page="true"]')?.click();
      });
      expect(container.querySelector('[data-browser-portal="true"]')).not.toBeNull();
      expect(container.querySelectorAll('[data-browser-page-tab="true"]')).toHaveLength(1);
      browserSessionStore.closeAll();
      await act(async () => root.render(<BrowserWorkspacePanel />));
      expect(container.querySelector('[data-browser-portal="true"]')).not.toBeNull();
      expect(container.querySelector('[data-browser-new-page="true"]')).not.toBeNull();
      expect(container.querySelectorAll('[data-browser-page-tab="true"]')).toHaveLength(0);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('opens a portal bookmark in the current blank page', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<BrowserWorkspacePanel />));
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-new-page="true"]')?.click();
      });
      await vi.waitFor(() => {
        expect(container.querySelector('[data-browser-bookmark="github"]')).not.toBeNull();
      });
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-bookmark="github"] button')?.click();
      });
      const pageId = browserSessionStore.snapshot().activePageId;
      expect(browserWebviewHost.commitNavigation).toHaveBeenCalledWith(pageId, 'https://github.com/');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('submits a typed address on a blank page and can open it in the system browser', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<BrowserWorkspacePanel />));
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-new-page="true"]')?.click();
      });
      const address = container.querySelector<HTMLInputElement>('[data-browser-address="true"]');
      expect(address).not.toBeNull();
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
        setter?.call(address, 'baidu.com');
        address!.dispatchEvent(new Event('input', { bubbles: true }));
      });
      await act(async () => {
        address!.form?.requestSubmit();
      });
      const pageId = browserSessionStore.snapshot().activePageId;
      expect(browserWebviewHost.commitNavigation).toHaveBeenCalledWith(pageId, 'https://baidu.com');
      const openExternal = container.querySelector<HTMLButtonElement>('[data-browser-open-external="true"]');
      expect(openExternal?.disabled).toBe(false);
      expect(container.textContent).toContain('用系统浏览器打开');
      await act(async () => openExternal?.click());
      expect(openExternalUrl).toHaveBeenCalledWith('https://baidu.com');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the typed address draft when the open page reports a new url', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      browserSessionStore.openUrl('https://example.com/');
      await act(async () => root.render(<BrowserWorkspacePanel />));
      const address = container.querySelector<HTMLInputElement>('[data-browser-address="true"]');
      expect(address).not.toBeNull();

      await act(async () => {
        address?.focus();
      });
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
        setter?.call(address, 'manage');
        address!.dispatchEvent(new Event('input', { bubbles: true }));
      });
      expect(address?.value).toBe('manage');

      // Pages keep pushing url updates (redirects, SPA routes) while the user types.
      await act(async () => {
        const pageId = browserSessionStore.snapshot().activePageId;
        browserSessionStore.commitPageUrl(pageId as string, 'https://example.com/next');
      });

      expect(address?.value).toBe('manage');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('toggles the current page between desktop and mobile site from the toolbar', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      browserSessionStore.openUrl('https://example.com/');
      await act(async () => root.render(<BrowserWorkspacePanel />));
      const toggle = container.querySelector<HTMLButtonElement>('[data-browser-view-mode="desktop"]');
      expect(toggle).not.toBeNull();
      expect(toggle?.disabled).toBe(false);
      expect(container.textContent).toContain('切换到移动版');
      await act(async () => toggle?.click());
      const pageId = browserSessionStore.snapshot().activePageId;
      expect(browserWebviewHost.setViewMode).toHaveBeenCalledWith(pageId, 'mobile');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('disables site chrome on the portal and toggles the current origin in bookmarks', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<BrowserWorkspacePanel />));
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-new-page="true"]')?.click();
      });
      expect(container.querySelector<HTMLButtonElement>('[data-browser-view-mode="desktop"]')?.disabled).toBe(true);
      expect(container.querySelector<HTMLButtonElement>('[data-browser-bookmark-toggle="true"]')?.disabled).toBe(true);

      browserSessionStore.openUrl('https://github.com/diodeme/Gold-Band');
      await act(async () => root.render(<BrowserWorkspacePanel />));
      await vi.waitFor(() => {
        expect(container.querySelector('[data-browser-bookmark-toggle="true"]')?.getAttribute('data-browser-bookmarked')).toBe('true');
      });
      expect(container.querySelector('[data-browser-bookmark-toggle="true"] svg')?.classList.contains('fill-current')).toBe(true);
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-bookmark-toggle="true"]')?.click();
      });
      expect(container.querySelector('[data-browser-bookmark-toggle="true"]')?.getAttribute('data-browser-bookmarked')).toBe('false');
      expect(container.querySelector('[data-browser-bookmark-toggle="true"] svg')?.classList.contains('fill-current')).toBe(false);
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-bookmark-toggle="true"]')?.click();
      });
      expect(container.querySelector('[data-browser-bookmark-toggle="true"]')?.getAttribute('data-browser-bookmarked')).toBe('true');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows the origin favicon on an internal page tab', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      browserSessionStore.openUrl('https://github.com/diodeme/Gold-Band');
      browserHistoryStore.replaceForTests([{
        url: 'https://github.com/diodeme/Gold-Band',
        title: 'GitHub',
        origin: 'https://github.com',
        lastVisitedAt: 1,
        faviconDataUrl: 'data:image/png;base64,aaa',
      }]);
      await act(async () => root.render(<BrowserWorkspacePanel />));
      expect(container.querySelector('[data-browser-page-tab="true"] [data-browser-favicon="true"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
