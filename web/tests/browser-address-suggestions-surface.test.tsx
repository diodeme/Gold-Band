/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const tauri = vi.hoisted(() => ({ invoke: vi.fn(async () => undefined) }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: tauri.invoke }));

import { BrowserAddressSuggestionsSurface } from '@/browser-address-suggestions-main';
import type { BrowserAddressSuggestionOverlayInput } from '@/api/client';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const state: BrowserAddressSuggestionOverlayInput = {
  revision: 42,
  bounds: { x: 10, y: 20, width: 320, height: 72 },
  activeIndex: null,
  items: [
    {
      key: 'visit:https://github.com/',
      kind: 'visit',
      title: 'GitHub',
      detail: 'github.com',
      faviconDataUrl: null,
      removeLabel: 'Remove',
    },
  ],
  theme: {
    dark: false,
    themeId: 'builtin.gold-band',
    colorScheme: 'light',
    visualQuality: 'full',
    materialModel: 'solid',
    variables: {},
  },
};

afterEach(() => {
  tauri.invoke.mockClear();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
  delete (window as { __GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__?: unknown }).__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__;
});

async function renderSurface() {
  (window as { __GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__?: unknown }).__GOLD_BAND_BROWSER_ADDRESS_SUGGESTIONS__ = state;
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<BrowserAddressSuggestionsSurface />);
  });
  return { container, root };
}

describe('browser address suggestions surface', () => {
  it('reports readiness even when a hidden webview does not schedule animation frames', async () => {
    const requestAnimationFrame = vi.fn(() => 1);
    vi.stubGlobal('requestAnimationFrame', requestAnimationFrame);
    vi.stubGlobal('cancelAnimationFrame', vi.fn());

    const { root } = await renderSurface();

    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(tauri.invoke).toHaveBeenCalledWith('browser_address_suggestions_ready', {
      input: { revision: 42 },
    });
    await act(async () => root.unmount());
  });

  it('reports the committed revision so the host can reveal the overlay safely', async () => {
    const { root } = await renderSurface();

    await vi.waitFor(() => {
      expect(tauri.invoke).toHaveBeenCalledWith('browser_address_suggestions_ready', {
        input: { revision: 42 },
      });
    });
    await act(async () => root.unmount());
  });

  it('forwards a chosen suggestion through the validated action command', async () => {
    const { container, root } = await renderSurface();
    const row = container.querySelector<HTMLElement>('[data-browser-history-item="visit"]');
    expect(row).not.toBeNull();

    await act(async () => {
      row?.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, cancelable: true }));
    });

    expect(tauri.invoke).toHaveBeenCalledWith('browser_address_suggestion_action', {
      input: { revision: 42, kind: 'choose', key: 'visit:https://github.com/' },
    });
    await act(async () => root.unmount());
  });

  it('forwards the remove action without choosing the suggestion', async () => {
    const { container, root } = await renderSurface();
    const remove = container.querySelector<HTMLElement>('[data-browser-history-remove="true"]');
    expect(remove).not.toBeNull();

    await act(async () => {
      remove?.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, cancelable: true }));
    });

    expect(tauri.invoke).toHaveBeenCalledWith('browser_address_suggestion_action', {
      input: { revision: 42, kind: 'remove', key: 'visit:https://github.com/' },
    });
    expect(tauri.invoke.mock.calls.filter(
      ([command]) => command === 'browser_address_suggestion_action',
    )).toHaveLength(1);
    await act(async () => root.unmount());
  });
});
