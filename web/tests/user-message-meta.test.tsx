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
    expect(role?.textContent).toContain('/Development-and-Testing');
    expect(role?.querySelector('img')?.getAttribute('src')).toBe('/logo.svg');
    expect(quotes).toBeTruthy();
    expect(Boolean(role && quotes && (role.compareDocumentPosition(quotes) & Node.DOCUMENT_POSITION_FOLLOWING)))
      .toBe(true);
  });

  it('scrolls the full role content in the hover tooltip instead of the summary', async () => {
    const content = `第一段角色定义\n${'很长的角色正文。'.repeat(80)}`;
    await act(async () => root.render(
      <SlashCommandInputTag
        prefix="/dev"
        description="short summary"
        content={content}
        kind="role"
        iconSrc="/logo.svg"
      />,
    ));

    const trigger = host.querySelector<HTMLButtonElement>('[data-slash-tag-kind="role"]');
    expect(trigger).toBeTruthy();
    await act(async () => {
      trigger!.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    const tooltip = document.body.querySelector('[data-slash-role-tooltip="true"]');
    expect(tooltip).toBeTruthy();
    expect(tooltip?.className).toContain('overflow-y-auto');
    expect(tooltip?.className).toContain('max-h-64');
    expect(tooltip?.textContent).toBe(content);
    expect(tooltip?.textContent).not.toContain('short summary');
  });
});
