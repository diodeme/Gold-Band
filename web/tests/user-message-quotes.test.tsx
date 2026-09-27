// @vitest-environment jsdom

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';

import '../src/i18n';
import { UserMessageQuotes } from '../src/components/conversation/UserMessageQuotes';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

describe('UserMessageQuotes', () => {
  it('does not render an entry for messages without quotes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => root.render(<UserMessageQuotes quotes={[]} />));
    expect(container.querySelector('[data-user-message-quotes-trigger]')).toBeNull();
    await act(async () => root.unmount());
  });

  it('opens multiple quotes with a fixed header and an internally scrolling list', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const longText = `第一行\n${'很长的引用内容'.repeat(100)}`;

    try {
      await act(async () => root.render(
        <UserMessageQuotes quotes={[
          { id: 'quote-1', text: longText, source: { kind: 'agentMessage', messageKey: 'textDelta-message-1' } },
          { id: 'quote-2', text: '第二条引用', source: { kind: 'agentMessage', messageKey: 'textDelta-message-2' } },
        ]} />,
      ));
      const trigger = container.querySelector<HTMLButtonElement>('[data-user-message-quotes-trigger]');
      expect(trigger?.textContent).toContain('2 条引用');

      await act(async () => trigger!.click());
      const popover = document.body.querySelector<HTMLElement>('[data-user-message-quotes-popover]');
      const scroll = document.body.querySelector<HTMLElement>('[data-user-message-quotes-scroll]');
      expect(popover).not.toBeNull();
      expect(popover?.firstElementChild?.className).toContain('max-h-[min(24rem,calc(100vh-4rem))]');
      expect(scroll?.className).toContain('overflow-y-auto');
      expect(scroll?.previousElementSibling?.className).toContain('shrink-0');
      expect(scroll?.textContent).toContain(longText);
      expect(scroll?.textContent).toContain('第二条引用');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('names file and diff sources and does not show the text of a whole-file diff', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <UserMessageQuotes quotes={[
          { id: 'file', text: 'const a = 1;', source: { kind: 'file', label: 'src/a.ts', startLine: 3, endLine: 5 } },
          { id: 'diff', text: '', source: { kind: 'diff', path: 'src/b.ts', origin: 'commit', revision: 'abc123', scope: 'file' } },
        ]} />,
      ));
      await act(async () => container.querySelector<HTMLButtonElement>('[data-user-message-quotes-trigger]')!.click());
      const scroll = document.body.querySelector<HTMLElement>('[data-user-message-quotes-scroll]');
      const items = Array.from(scroll?.children ?? []);
      expect(items[0]?.textContent).toContain('src/a.ts 第 3-5 行');
      expect(items[0]?.textContent).toContain('const a = 1;');
      expect(items[1]?.textContent).toBe('src/b.ts 完整 diff · 提交 abc123');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
