/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useAcpComposerDraft } from '@/lib/acp-composer-draft';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function DraftProbe({ draftKey }: { draftKey: string }) {
  const { draft, setContent, setAttachments } = useAcpComposerDraft(draftKey);
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
    });
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');

    await renderSession(secondKey);
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('');
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('0');

    await renderSession(firstKey);
    expect((host.querySelector('input') as HTMLInputElement).value).toBe('未发送的追问');
    expect(host.querySelector('[data-testid="attachment-count"]')?.textContent).toBe('1');
  });
});
