/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const host = vi.hoisted(() => ({
  ensurePage: vi.fn(async () => undefined),
  scheduleBounds: vi.fn(),
  hideAll: vi.fn(async () => undefined),
  onOverlayChange: vi.fn(() => () => undefined),
  onVisibilityChange: vi.fn(() => () => undefined),
}));

vi.mock('@/components/workspace/browser/browser-webview-host', () => ({
  browserWebviewHost: host,
}));

import { NativeBrowserViewport } from '@/components/workspace/browser/NativeBrowserViewport';
import type { BrowserPage } from '@/components/workspace/browser/browser-session-store';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserver);

const page: BrowserPage = {
  pageId: 'page-1',
  url: 'https://example.com/',
  title: 'Example',
  loading: false,
  live: true,
  viewMode: 'desktop',
};

afterEach(() => {
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('NativeBrowserViewport', () => {
  it('shows the retained native page before the first frame so returning to it cannot paint an empty pane', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const frames: FrameRequestCallback[] = [];
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      expect(host.ensurePage).toHaveBeenCalled();
      expect(frames).toHaveLength(0);
    } finally {
      await act(async () => root.unmount());
      container.remove();
      vi.stubGlobal('requestAnimationFrame', originalRequestAnimationFrame);
    }
  });

  it('shows the native page while it is still loading so progressive content is not hidden behind the loader', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const loadingPage = { ...page, loading: true };
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={loadingPage} visible loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalledWith(
        loadingPage,
        expect.any(Object),
        true,
      ));
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('resyncs bounds when the native page becomes live without hiding it', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const creating = { ...page, live: false, loading: true };
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={creating} visible loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalled());
      host.hideAll.mockClear();
      host.ensurePage.mockClear();
      host.scheduleBounds.mockClear();
      await act(async () => {
        root.render(
          <NativeBrowserViewport
            page={{ ...creating, live: true, loading: true }}
            visible
            loadingLabel="Loading page"
          />,
        );
      });
      expect(host.hideAll).not.toHaveBeenCalled();
      await vi.waitFor(() => {
        expect(host.ensurePage.mock.calls.length + host.scheduleBounds.mock.calls.length).toBeGreaterThan(0);
      });
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('does not hide the native page when only title or loading changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalled());
      host.hideAll.mockClear();
      host.ensurePage.mockClear();
      await act(async () => {
        root.render(
          <NativeBrowserViewport
            page={{ ...page, title: 'Updated title', loading: true }}
            visible
            loadingLabel="Loading page"
          />,
        );
      });
      expect(host.hideAll).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('hides the previous native page when the page identity changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalled());
      host.hideAll.mockClear();
      await act(async () => {
        root.render(
          <NativeBrowserViewport
            page={{ ...page, pageId: 'page-2', url: 'https://example.com/two' }}
            visible
            loadingLabel="Loading page"
          />,
        );
      });
      expect(host.hideAll).toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('resyncs the native page when host visibility becomes showable again', async () => {
    let visibilityListener: (() => void) | undefined;
    host.onVisibilityChange.mockImplementation((listener: () => void) => {
      visibilityListener = listener;
      return () => {
        if (visibilityListener === listener) visibilityListener = undefined;
      };
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalled());
      host.ensurePage.mockClear();
      await act(async () => {
        visibilityListener?.();
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalled());
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('keeps the native host bounds unchanged while the address overlay is open', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      const hostNode = container.querySelector<HTMLElement>('[data-browser-native-host="true"]');
      expect(hostNode?.style.top).toBe('');
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('keeps measuring placeholder bounds while the portal covers the native host', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const blankPage = {
      ...page,
      url: 'about:blank',
      title: 'New tab',
      loading: false,
      live: false,
    };
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={blankPage} visible={false} loadingLabel="Loading page" />);
      });
      await vi.waitFor(() => expect(host.ensurePage).toHaveBeenCalledWith(
        blankPage,
        expect.any(Object),
        false,
      ));
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('hides the native webview as soon as the placeholder unmounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<NativeBrowserViewport page={page} visible loadingLabel="Loading page" />);
      });
      await act(async () => {
        root.unmount();
      });
      expect(host.hideAll).toHaveBeenCalled();
    } finally {
      container.remove();
    }
  });
});
