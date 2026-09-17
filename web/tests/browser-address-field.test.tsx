/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { BrowserAddressField } from '@/components/workspace/browser/BrowserAddressField';
import { browserHistoryStore } from '@/components/workspace/browser/browser-history-store';

const historyApi = vi.hoisted(() => {
  const initial = [
    {
      url: 'https://github.com/diodeme/Gold-Band',
      title: 'Sign in to GitHub · GitHub',
      origin: 'https://github.com',
      lastVisitedAt: 2,
      faviconDataUrl: 'data:image/png;base64,aaa',
    },
    {
      url: 'https://www.baidu.com/',
      title: '百度一下，你就知道',
      origin: 'https://www.baidu.com',
      lastVisitedAt: 1,
    },
  ];
  let visits = initial.map((visit) => ({ ...visit }));
  return {
    initial,
    snapshot: () => visits,
    reset() {
      visits = initial.map((visit) => ({ ...visit }));
    },
    list: async () => visits,
    remove: async (input: { url: string }) => {
      visits = visits.filter((visit) => visit.url !== input.url);
      return visits;
    },
  };
});

const nativeOverlayCalls = vi.hoisted(() => ({
  show: [] as Array<Record<string, unknown>>,
  hide: [] as Array<Record<string, unknown>>,
  reports: [] as Array<Record<string, unknown>>,
  showError: null as unknown,
  actionListener: null as unknown,
  reset() {
    this.show = [];
    this.hide = [];
    this.reports = [];
    this.showError = null;
    this.actionListener = null;
  },
}));

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({
    browserListHistory: historyApi.list,
    browserDeleteHistory: historyApi.remove,
    browserShowAddressSuggestions: async (input: Record<string, unknown>) => {
      nativeOverlayCalls.show.push(input);
      if (nativeOverlayCalls.showError) throw nativeOverlayCalls.showError;
    },
    browserHideAddressSuggestions: async (input: Record<string, unknown>) => {
      nativeOverlayCalls.hide.push(input);
    },
    reportFrontendError: async (input: Record<string, unknown>) => {
      nativeOverlayCalls.reports.push(input);
    },
    subscribeBrowserAddressSuggestionActions: async (listener: unknown) => {
      nativeOverlayCalls.actionListener = listener;
      return () => undefined;
    },
  }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverStub);

afterEach(() => {
  historyApi.reset();
  nativeOverlayCalls.reset();
  browserHistoryStore.resetForTests();
  document.body.replaceChildren();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

function fieldProps(overrides: Partial<React.ComponentProps<typeof BrowserAddressField>> = {}) {
  return {
    address: '',
    typed: false,
    onAddressChange: () => undefined,
    onTyped: () => undefined,
    onSubmit: () => undefined,
    ...overrides,
  };
}

describe('BrowserAddressField', () => {
  it('requests the native suggestion surface on focus in the desktop runtime', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const request = nativeOverlayCalls.show.at(-1) ?? {};
      expect(request.items).toHaveLength(2);
      expect((request.bounds as { width: number }).width).toBeGreaterThan(0);
      expect((request.revision as number)).toBeGreaterThan(0);
      expect(container.querySelector('[data-browser-history="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('sends one native overlay request per projection change', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const sent = nativeOverlayCalls.show.length;

      // Re-rendering and resync events with an unchanged projection must not re-issue
      // the native request: re-issuing it is what raced concurrent overlay creates.
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        window.dispatchEvent(new Event('gold-band-theme-icons-changed'));
      });
      expect(nativeOverlayCalls.show).toHaveLength(sent);

      // A real projection change still reaches the native overlay exactly once.
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ address: 'git', typed: true })} />);
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show).toHaveLength(sent + 1);
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('reports a structured native overlay failure and retries on the next projection', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    nativeOverlayCalls.showError = { code: 'browser.webview.create_failed', params: { reason: 'label' } };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.reports.length).toBeGreaterThan(0);
      });
      const report = nativeOverlayCalls.reports.at(-1) ?? {};
      expect(String(report.message)).toContain('browser.webview.create_failed');
      const failedAttempts = nativeOverlayCalls.show.length;

      nativeOverlayCalls.showError = null;
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ address: 'git', typed: true })} />);
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show).toHaveLength(failedAttempts + 1);
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('honors a suggestion click that arrives after the blur-triggered hide', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSubmit = vi.fn();
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ onSubmit })} />);
      });
      const input = container.querySelector<HTMLInputElement>('[data-browser-address="true"]');
      await act(async () => {
        input?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const displayed = nativeOverlayCalls.show.at(-1) ?? {};
      const revision = displayed.revision as number;

      // Clicking the overlay blurs the address input, but the suggestion session stays
      // open so choose can still use the displayed revision.
      await act(async () => {
        input?.blur();
      });
      await act(async () => {
        (nativeOverlayCalls.actionListener as (event: unknown) => void)({
          revision,
          kind: 'choose',
          key: 'visit:https://github.com/diodeme/Gold-Band',
        });
      });

      expect(onSubmit).toHaveBeenCalledWith('https://github.com/diodeme/Gold-Band');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not hide the native suggestion overlay when a visit is removed after blur', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSubmit = vi.fn();
    const onEditingChange = vi.fn();
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ onSubmit, onEditingChange })} />);
      });
      const input = container.querySelector<HTMLInputElement>('[data-browser-address="true"]');
      await act(async () => {
        input?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const displayed = nativeOverlayCalls.show.at(-1) ?? {};
      const revision = displayed.revision as number;
      const hideBeforeRemove = nativeOverlayCalls.hide.length;

      // Overlay clicks blur the address field first. Hiding here is what makes the
      // remaining records flash: the overlay would hide, then show again after delete.
      await act(async () => {
        input?.blur();
      });
      expect(nativeOverlayCalls.hide).toHaveLength(hideBeforeRemove);

      await act(async () => {
        (nativeOverlayCalls.actionListener as (event: unknown) => void)({
          revision,
          kind: 'remove',
          key: 'visit:https://github.com/diodeme/Gold-Band',
        });
      });

      await vi.waitFor(() => {
        const latest = nativeOverlayCalls.show.at(-1) ?? {};
        expect(nativeOverlayCalls.hide).toHaveLength(hideBeforeRemove);
        expect(latest.items).toHaveLength(1);
        expect((latest.items as Array<{ key: string }>)[0]?.key).toBe('visit:https://www.baidu.com/');
        expect(document.activeElement).toBe(input);
      });
      expect(onSubmit).not.toHaveBeenCalled();
      expect(onEditingChange).toHaveBeenLastCalledWith(true);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('hides the native suggestion overlay when the user clicks outside the address field', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const hideBefore = nativeOverlayCalls.hide.length;

      await act(async () => {
        document.body.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
      });

      await vi.waitFor(() => {
        expect(nativeOverlayCalls.hide.length).toBeGreaterThan(hideBefore);
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('hides the native suggestion overlay when the browsing page takes focus', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps()} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await vi.waitFor(() => {
        expect(nativeOverlayCalls.show.length).toBeGreaterThan(0);
      });
      const displayed = nativeOverlayCalls.show.at(-1) ?? {};
      const hideBefore = nativeOverlayCalls.hide.length;

      await act(async () => {
        (nativeOverlayCalls.actionListener as (event: unknown) => void)({
          revision: displayed.revision,
          kind: 'dismiss',
          key: 'page-focus',
        });
      });

      await vi.waitFor(() => {
        expect(nativeOverlayCalls.hide.length).toBeGreaterThan(hideBefore);
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows recent visits on focus before the user types', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSubmit = vi.fn();
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ onSubmit })} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      const items = container.querySelectorAll('[data-browser-history-item]');
      expect(items).toHaveLength(2);
      expect(container.querySelector('[data-browser-history="true"]')?.className).toContain('absolute');
      expect(items[0]?.getAttribute('data-browser-history-item')).toBe('visit');
      expect(container.textContent).toContain('Sign in to GitHub');
      await act(async () => {
        (items[0] as HTMLElement).click();
      });
      expect(onSubmit).toHaveBeenCalledWith('https://github.com/diodeme/Gold-Band');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not reopen recents after a typed address is submitted', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSubmit = vi.fn();
    try {
      await act(async () => {
        root.render(
          <BrowserAddressField
            {...fieldProps({
              address: 'https://windows.do/',
              typed: true,
              onSubmit,
            })}
          />,
        );
      });
      const input = container.querySelector<HTMLInputElement>('[data-browser-address="true"]');
      await act(async () => {
        input?.focus();
      });
      await act(async () => {
        input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
      });
      expect(onSubmit).toHaveBeenCalledTimes(1);
      await act(async () => {
        root.render(
          <BrowserAddressField
            {...fieldProps({
              address: 'https://windows.do/',
              typed: false,
              onSubmit,
            })}
          />,
        );
      });
      expect(container.querySelector('[data-browser-history="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('removes a visit from history without navigating', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSubmit = vi.fn();
    try {
      await act(async () => {
        root.render(<BrowserAddressField {...fieldProps({ onSubmit })} />);
      });
      await act(async () => {
        container.querySelector<HTMLInputElement>('[data-browser-address="true"]')?.focus();
      });
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-browser-history-remove="true"]')?.click();
      });
      await vi.waitFor(() => {
        expect(container.querySelectorAll('[data-browser-history-item="visit"]')).toHaveLength(1);
      });
      expect(container.querySelector('[data-browser-history="true"]')).not.toBeNull();
      expect(container.textContent).not.toContain('Sign in to GitHub');
      expect(onSubmit).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
