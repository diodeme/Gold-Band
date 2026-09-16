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

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({
    browserListHistory: historyApi.list,
    browserDeleteHistory: historyApi.remove,
  }),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  historyApi.reset();
  browserHistoryStore.resetForTests();
  document.body.replaceChildren();
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
