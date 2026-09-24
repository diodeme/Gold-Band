/** @vitest-environment jsdom */

import React, { act, useEffect, useLayoutEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const host = vi.hoisted(() => ({
  discardAll: vi.fn(async () => undefined),
  hideAll: vi.fn(async () => undefined),
  suppress: vi.fn(async () => undefined),
  resume: vi.fn(),
  hasBlockingOverlay: vi.fn(() => false),
}));

vi.mock('@/components/workspace/browser/browser-webview-host', () => ({
  browserWebviewHost: host,
}));

import {
  BrowserNativeLifecycle,
  resolveBrowserResourceTransition,
} from '@/components/workspace/browser/browser-workspace-hooks';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('BrowserNativeLifecycle', () => {
  it('discards only an explicitly closed browser resource', async () => {
    await resolveBrowserResourceTransition('workspace-close');
    expect(host.suppress).toHaveBeenCalledTimes(1);
    expect(host.discardAll).not.toHaveBeenCalled();
    vi.clearAllMocks();

    await resolveBrowserResourceTransition('close');
    expect(host.discardAll).toHaveBeenCalledTimes(1);
    expect(host.suppress).not.toHaveBeenCalled();
  });

  it('does not discard after a brief unavailable window if the workspace is available again', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const props = {
      scopeKey: 'conversation-a',
      presented: true,
      autoCollapsedHidden: false,
      requestedOpen: true,
      activeIsBrowser: true,
    };
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} available />);
    });
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} available={false} />);
      root.render(<BrowserNativeLifecycle {...props} available />);
    });
    await act(async () => undefined);
    expect(host.discardAll).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it('suppresses but retains child webviews when the right workspace stays unavailable', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const props = {
      scopeKey: 'conversation-a',
      presented: true,
      autoCollapsedHidden: false,
      requestedOpen: true,
      activeIsBrowser: true,
    };
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} available />);
    });
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} available={false} />);
    });
    await vi.waitFor(() => expect(host.suppress).toHaveBeenCalledTimes(1));
    expect(host.discardAll).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it('suppresses but retains child webviews when the owning workspace shell unmounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <BrowserNativeLifecycle
          scopeKey="conversation-a"
          presented
          autoCollapsedHidden={false}
          available
          requestedOpen
          activeIsBrowser
        />,
      );
    });
    await act(async () => root.unmount());
    await vi.waitFor(() => expect(host.suppress).toHaveBeenCalled());
    expect(host.discardAll).not.toHaveBeenCalled();
  });

  it('hides the native page during layout before the next page lays out or runs passive effects', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const props = {
      scopeKey: 'conversation-a',
      presented: true,
      autoCollapsedHidden: false,
      requestedOpen: true,
      activeIsBrowser: true,
    };
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} available />);
    });
    await vi.waitFor(() => expect(host.resume).toHaveBeenCalled());
    const order: string[] = [];
    host.suppress.mockImplementation(() => {
      order.push('suppress');
      return Promise.resolve();
    });
    function NextPage() {
      useLayoutEffect(() => {
        order.push('later-layout');
      });
      useEffect(() => {
        order.push('passive');
      });
      return null;
    }
    await act(async () => {
      root.render(
        <>
          <BrowserNativeLifecycle {...props} available={false} />
          <NextPage />
        </>,
      );
    });
    expect(order).toEqual(['suppress', 'later-layout', 'passive']);
    expect(host.discardAll).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it('does not hide when strict mode replays the owner mount', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <React.StrictMode>
          <BrowserNativeLifecycle
            scopeKey="conversation-a"
            presented
            autoCollapsedHidden={false}
            available
            requestedOpen
            activeIsBrowser
          />
        </React.StrictMode>,
      );
    });
    await act(async () => undefined);
    expect(host.suppress).not.toHaveBeenCalled();
    expect(host.discardAll).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it('keeps the retained webview visible when only the scope identity changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const props = {
      presented: true,
      autoCollapsedHidden: false,
      available: true,
      requestedOpen: true,
      activeIsBrowser: true,
    };
    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} scopeKey="conversation-a" />);
    });
    await vi.waitFor(() => expect(host.resume).toHaveBeenCalled());
    vi.clearAllMocks();

    await act(async () => {
      root.render(<BrowserNativeLifecycle {...props} scopeKey="conversation-b" />);
    });

    await act(async () => undefined);
    expect(host.suppress).not.toHaveBeenCalled();
    expect(host.discardAll).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });
});
