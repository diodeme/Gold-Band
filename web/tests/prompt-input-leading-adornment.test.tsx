/** @vitest-environment jsdom */
import { act, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PromptInput, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import { useSlashCommandController } from '@/hooks/useSlashCommandController';
import { parseCommittedSlashItem, roleSlashItems, type SlashCatalogGroup } from '@/lib/slash-command';

const roleGroup: SlashCatalogGroup[] = [{
  id: 'roles',
  heading: '',
  items: roleSlashItems([{ id: 'pf-dev', name: '开发', summary: 'dev', content: 'role body' }]),
}];

function Harness({ initial }: { initial: string }) {
  const [input, setInput] = useState(initial);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const slash = useSlashCommandController({ input, groups: roleGroup, onInputChange: setInput, textareaRef });
  const committed = useMemo(
    () => parseCommittedSlashItem(input, slash.catalogItems, slash.selectedIdentity),
    [input, slash.catalogItems, slash.selectedIdentity],
  );
  return (
    <PromptInput value={input} onValueChange={setInput}>
      <PromptInputTextarea
        ref={textareaRef}
        valuePrefix={committed?.prefix}
        leadingAdornment={committed ? <span data-tag>{committed.prefix}</span> : null}
        onKeyDown={slash.onKeyDown}
      />
    </PromptInput>
  );
}

let cleanup = async () => {};
afterEach(async () => {
  await cleanup();
  vi.unstubAllGlobals();
});

async function mount(initial: string) {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    disconnect() {}
  });
  const host = document.createElement('div');
  document.body.append(host);
  const root = createRoot(host);
  cleanup = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  await act(async () => root.render(<Harness initial={initial} />));
  return host;
}

describe('PromptInputTextarea leading adornment', () => {
  it('keeps the focused textarea when Backspace turns the role tag back into text', async () => {
    const host = await mount('@开发 ');
    const textarea = host.querySelector('textarea')!;
    expect(host.querySelector('[data-tag]')).not.toBeNull();
    expect(textarea.value).toBe(' ');

    textarea.focus();
    textarea.setSelectionRange(1, 1);
    await act(async () => {
      textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }));
    });

    expect(host.querySelector('[data-tag]')).toBeNull();
    expect(host.querySelector('textarea')).toBe(textarea);
    expect(document.activeElement).toBe(textarea);
    expect(textarea.value).toBe('@开发');
    expect(textarea.selectionStart).toBe('@开发'.length);
  });

  it('keeps the focused textarea when a role tag is committed', async () => {
    const host = await mount('@开发');
    const textarea = host.querySelector('textarea')!;
    textarea.focus();
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
      setter.call(textarea, '@开发 ');
      textarea.dispatchEvent(new Event('input', { bubbles: true }));
    });

    expect(host.querySelector('[data-tag]')).not.toBeNull();
    expect(host.querySelector('textarea')).toBe(textarea);
    expect(document.activeElement).toBe(textarea);
  });
});
