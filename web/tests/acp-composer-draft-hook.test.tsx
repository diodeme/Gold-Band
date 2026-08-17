/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useAcpComposerDraft } from '@/lib/acp-composer-draft';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function DraftProbe({ draftKey }: { draftKey: string }) {
  const controller = useAcpComposerDraft(draftKey);
  const { draft, setContent, setAttachments, setQuotes } = controller;
  return (
    <div>
      <input
        aria-label="draft"
        value={draft.content}
        onChange={(event) => setContent(event.target.value)}
      />
      <button
        type="button"
        onClick={() => setAttachments([{ id: draftKey, name: 'image.png', size: 1, mime: 'image/png', source: 'dialog' }])}
      >
        attach
      </button>
      <span data-testid="attachment-count">{draft.attachments.length}</span>
      <button type="button" onClick={() => setQuotes([{ id: 'quote', sourceKey: 'message', text: '引用' }])}>quote</button>
      <span data-testid="quote-count">{draft.quotes.length}</span>
      <button type="button" onClick={() => controller.clearIfUnchanged(draft)}>detach</button>
      <button
        type="button"
        onClick={() => controller.restoreIfEmpty({
          content: '发送失败后恢复',
          attachments: [{ id: 'restored', name: 'restored.png', size: 1, mime: 'image/png', source: 'dialog' }],
          quotes: [{ id: 'restored-quote', sourceKey: 'message', text: '恢复引用' }],
        })}
      >
        restore
      </button>
      <button
        type="button"
        onClick={() => controller.replaceIfUnchanged(draft, {
          ...draft,
          attachments: [{ id: 'enriched', name: 'enriched.png', size: 12, mime: 'image/png', source: 'dialog' }],
        })}
      >
        enrich
      </button>
    </div>
  );
}

describe('useAcpComposerDraft session switching contract', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function renderSession(draftKey: string) {
    await act(async () => root.render(<DraftProbe draftKey={draftKey} />));
  }

  it('restores the original text and attachment after switching to another keyed session', async () => {
    const firstKey = `first-${crypto.randomUUID()}`;
    const secondKey = `second-${crypto.randomUUID()}`;
    await renderSession(firstKey);

    const firstInput = host.querySelector('input')!;
    await act(async () => {
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      valueSetter.call(firstInput, '未发送的追问');
      firstInput.dispatchEvent(new Event('input', { bubbles: true }));
      host.querySelector('button')!.click();
      (host.querySelectorAll('button')[1] as HTMLButtonElement).click();
    });
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');
    expect(host.querySelector('[data-testid="quote-count"]')?.textContent).toBe('1');

    await act(async () => (host.querySelectorAll('button')[4] as HTMLButtonElement).click());
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');

    await renderSession(secondKey);
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('');
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('0');
    expect(host.querySelector('[data-testid="quote-count"]')?.textContent).toBe('0');

    await renderSession(firstKey);
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('未发送的追问');
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');
    expect(host.querySelector('[data-testid="quote-count"]')?.textContent).toBe('1');
  });

  it('restores a detached submission only while the composer is still empty', async () => {
    await renderSession(`restore-${crypto.randomUUID()}`);
    const buttons = host.querySelectorAll('button');
    const input = host.querySelector('input') as HTMLInputElement;
    const setInput = async (value: string) => act(async () => {
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
      valueSetter.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });

    await setInput('待发送');
    await act(async () => (buttons[2] as HTMLButtonElement).click());
    expect(input.value).toBe('');
    await act(async () => (buttons[3] as HTMLButtonElement).click());
    expect(input.value).toBe('发送失败后恢复');
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');
    expect(host.querySelector('[data-testid="quote-count"]')?.textContent).toBe('1');

    await setInput('用户的新输入');
    await act(async () => (buttons[3] as HTMLButtonElement).click());
    expect(input.value).toBe('用户的新输入');
  });
});
