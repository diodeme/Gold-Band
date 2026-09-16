/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { BrowserSettings } from '@/components/settings/BrowserSettings';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

describe('BrowserSettings', () => {
  it('exposes the search engine and both link-routing preferences through shadcn controls', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onChange = vi.fn();
    try {
      await act(async () => root.render(
        <BrowserSettings
          preferences={{
            schemaVersion: 1,
            searchEngine: 'baidu',
            openLocalLinksInBrowser: true,
            openWebLinksInBrowser: false,
          }}
          onChange={onChange}
        />,
      ));
      expect(container.textContent).toContain('搜索引擎');
      expect(container.textContent).toContain('Baidu');
      const local = container.querySelector<HTMLButtonElement>('#browser-open-local-links');
      const web = container.querySelector<HTMLButtonElement>('#browser-open-web-links');
      expect(local?.getAttribute('data-state')).toBe('checked');
      expect(web?.getAttribute('data-state')).toBe('unchecked');
      await act(async () => web?.click());
      expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ openWebLinksInBrowser: true }));
    } finally {
      await act(async () => root.unmount());
    }
  });
});
