/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { SlashCommandInputTag } from '@/components/conversation/SlashCommandInputTag';
import { UserMessageMeta } from '@/components/conversation/UserMessageMeta';
import '@/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('UserMessageMeta', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    document.body.replaceChildren();
    vi.unstubAllGlobals();
  });

  it('puts the role tag before quotes on one meta row', async () => {
    await act(async () => root.render(
      <UserMessageMeta
        role={{ profileId: 'pf-dev', name: 'Development and Testing', content: '完整角色定义' }}
        quotes={[{ id: 'quote-1', sourceMessageKey: 'message-1', text: '引用内容' }]}
      />,
    ));

    const meta = host.querySelector('[data-user-message-meta="true"]');
    const role = meta?.querySelector('[data-slash-tag-kind="role"]');
    const quotes = meta?.querySelector('[data-user-message-quotes-trigger="true"]');
    expect(meta?.className).toContain('flex');
    expect(role?.textContent).toBe('Development-and-Testing');
    expect(role?.textContent).not.toContain('/');
    expect(role?.querySelector('img')?.getAttribute('src')).toBe('/logo.svg');
    expect(quotes).toBeTruthy();
    expect(Boolean(role && quotes && (role.compareDocumentPosition(quotes) & Node.DOCUMENT_POSITION_FOLLOWING)))
      .toBe(true);
    expect(role?.className).toContain('h-7');
    expect(role?.className).toContain('rounded-full');
    expect(quotes?.className).toContain('h-7');
    expect(quotes?.className).toContain('rounded-full');
  });

  it('shows the committed tag name without a leading slash', async () => {
    await act(async () => root.render(
      <SlashCommandInputTag prefix="/dataviz" kind="command" iconSrc="/logo.svg" />,
    ));
    const tag = host.querySelector('[data-slot="slash-command-input-tag"]');
    expect(tag?.textContent).toBe('dataviz');
    expect(tag?.className).toContain('h-6');
    expect(tag?.className).not.toMatch(/(^|\s)h-7(\s|$)/);
    expect(tag?.querySelector('img')?.getAttribute('src')).toBe('/logo.svg');
  });
});
